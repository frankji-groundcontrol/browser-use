//! OpenAI-compatible chat client (used against OpenAI and the Sub2API gateway).

use std::{env, time::Duration};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::message::ChatMessage;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
// Matches Python's retry-path default (`llm_config.get('model', 'gpt-4o')`); a
// deployment against a gateway sets BROWSER_USE_LLM_MODEL to override this.
const DEFAULT_MODEL: &str = "gpt-4o";
// Python's ChatOpenAI default sampling temperature for the agent.
const DEFAULT_TEMPERATURE: f32 = 0.7;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
// Transient-failure retries, mirroring the OpenAI SDK's max_retries=5.
const MAX_RETRIES: usize = 5;

/// Minimal asynchronous OpenAI-compatible chat client.
#[derive(Debug, Clone)]
pub struct OpenAiChatClient {
    http: reqwest::Client,
    config: OpenAiChatConfig,
}

/// Runtime configuration loaded from environment variables.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenAiChatConfig {
    /// API key used as bearer auth.
    pub api_key: String,
    /// Base URL that already includes `/v1`.
    pub base_url: String,
    /// Chat completion model name.
    pub model: String,
    /// Optional sampling temperature.
    pub temperature: Option<f32>,
}

#[derive(Debug, Serialize, PartialEq)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
    // "length" means the model hit its output cap mid-token, so `content` is a
    // chopped prefix rather than an answer.
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    // Spec-valid responses (reasoning models, refusals, content filters, tool
    // calls) can send `content: null` or omit it; coerce to empty like Python's
    // `choice.message.content or ''` instead of failing to parse.
    #[serde(default)]
    content: Option<String>,
}

impl OpenAiChatConfig {
    /// Loads OpenAI-compatible chat configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_model_override(None)
    }

    /// Loads OpenAI-compatible chat configuration and applies an optional model override.
    pub fn from_env_with_model_override(model_override: Option<String>) -> Result<Self> {
        let api_key = env::var("OPENAI_API_KEY")
            .map(|value| value.trim().to_owned())
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("OPENAI_API_KEY is not set"))?;
        let base_url = env::var("OPENAI_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let model = model_override
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                env::var("BROWSER_USE_LLM_MODEL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let temperature = match env::var("BROWSER_USE_LLM_TEMPERATURE") {
            Ok(value) if !value.trim().is_empty() => Some(
                value
                    .parse::<f32>()
                    .with_context(|| format!("invalid BROWSER_USE_LLM_TEMPERATURE={value:?}"))?,
            ),
            // Default to Python's 0.7 rather than omitting it (which lets the
            // server apply its own, usually 1.0).
            _ => Some(DEFAULT_TEMPERATURE),
        };

        Ok(Self {
            api_key,
            base_url,
            model,
            temperature,
        })
    }

    fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

impl OpenAiChatClient {
    /// Creates a chat client from environment configuration.
    pub fn from_env() -> Result<Self> {
        Self::new(OpenAiChatConfig::from_env()?)
    }

    /// Creates a chat client from environment configuration and an optional model override.
    pub fn from_env_with_model_override(model_override: Option<String>) -> Result<Self> {
        Self::new(OpenAiChatConfig::from_env_with_model_override(
            model_override,
        )?)
    }

    /// Creates a chat client from explicit configuration.
    pub fn new(config: OpenAiChatConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { http, config })
    }

    /// Sends chat messages and returns the assistant message text.
    ///
    /// Retries transient failures (HTTP 429/5xx, connect/timeout) with
    /// exponential backoff, honoring `Retry-After`, mirroring the OpenAI SDK's
    /// `max_retries=5`. A `null`/empty assistant `content` returns an empty
    /// string rather than erroring (matching Python's `content or ''`).
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let request = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages,
            temperature: self.config.temperature,
        };

        let mut attempt = 0;
        loop {
            let send_result = self
                .http
                .post(self.config.chat_completions_url())
                .bearer_auth(&self.config.api_key)
                .json(&request)
                .send()
                .await;

            let response = match send_result {
                Ok(response) => response,
                Err(error) => {
                    // Connect/timeout/transport errors are transient.
                    if attempt < MAX_RETRIES && (error.is_timeout() || error.is_connect()) {
                        Self::backoff_sleep(attempt, None).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(anyhow::Error::new(error).context("LLM chat request failed"));
                }
            };

            let status = response.status();
            if status.is_success() {
                let body = response
                    .text()
                    .await
                    .context("failed to read LLM response body")?;
                return parse_chat_body(&body);
            }

            // 429 (rate limit) and 5xx are transient; retry with backoff.
            let retryable = status.as_u16() == 429 || status.is_server_error();
            if retryable && attempt < MAX_RETRIES {
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok());
                Self::backoff_sleep(attempt, retry_after).await;
                attempt += 1;
                continue;
            }

            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "LLM chat request failed with HTTP {status}: {body}"
            ));
        }
    }

    async fn backoff_sleep(attempt: usize, retry_after_secs: Option<u64>) {
        let delay = match retry_after_secs {
            Some(secs) => Duration::from_secs(secs.min(60)),
            None => {
                // 0.5s, 1s, 2s, 4s, 8s (capped) + small deterministic jitter.
                let base = 500u64.saturating_mul(1u64 << (attempt.min(4) as u32));
                let jitter = (attempt as u64 * 137) % 250;
                Duration::from_millis((base + jitter).min(15_000))
            }
        };
        tracing::debug!(
            attempt,
            ?delay,
            "retrying LLM request after transient failure"
        );
        tokio::time::sleep(delay).await;
    }
}

/// Extracts the assistant text from a successful chat-completions body. A
/// `null`, missing, or empty `content` yields an empty string (matching Python's
/// `content or ''`); only a genuinely empty `choices` array is an error.
fn parse_chat_body(body: &str) -> Result<String> {
    let parsed: ChatCompletionResponse =
        serde_json::from_str(body).context("failed to parse LLM chat response")?;
    let choice = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("LLM chat response had no choices"))?;
    // Reject a truncated completion before anyone parses or displays it: the
    // agent JSON-parses this text and extract_content shows it verbatim, so a
    // prefix is never an acceptable answer. Without this the failure surfaces as
    // a misleading JSON parse error and the same oversized prompt gets retried.
    if choice.finish_reason.as_deref() == Some("length") {
        return Err(anyhow!(
            "model output was truncated at the model's output token limit; the response is incomplete. Request shorter output or raise the server-side cap."
        ));
    }
    Ok(choice.message.content.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::parse_chat_body;

    #[test]
    fn null_content_yields_empty_string() {
        // Reasoning models / refusals / tool-call turns send content: null.
        let body = r#"{"choices":[{"message":{"role":"assistant","content":null}}]}"#;
        assert_eq!(parse_chat_body(body).unwrap(), "");
    }

    #[test]
    fn missing_content_yields_empty_string() {
        let body = r#"{"choices":[{"message":{"role":"assistant","tool_calls":[]}}]}"#;
        assert_eq!(parse_chat_body(body).unwrap(), "");
    }

    #[test]
    fn normal_content_is_returned() {
        let body = r#"{"choices":[{"message":{"content":"hello"}}]}"#;
        assert_eq!(parse_chat_body(body).unwrap(), "hello");
    }

    #[test]
    fn no_choices_is_an_error() {
        assert!(parse_chat_body(r#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn truncated_output_is_an_error_not_a_chopped_prefix() {
        // finish_reason "length" means the model hit its output cap mid-token, so
        // the content is a prefix. Callers either JSON-parse it (the agent) or show
        // it verbatim (extract_content), and both are wrong on a prefix: the parse
        // fails with a misleading "expected value" and the agent retries the same
        // oversized prompt. Fail with the real cause instead.
        let body = r#"{"choices":[{"finish_reason":"length","message":{"content":"{\"action\":\"cli"}}]}"#;
        let error = parse_chat_body(body).expect_err("truncated output must not be returned");
        let error = error.to_string();
        assert!(error.contains("truncated"), "got: {error}");
        // Never leak a "None" cap: this port sends no max_tokens to interpolate.
        assert!(!error.contains("None"), "got: {error}");
    }

    #[test]
    fn normal_finish_reason_is_not_treated_as_truncation() {
        let body = r#"{"choices":[{"finish_reason":"stop","message":{"content":"done"}}]}"#;
        assert_eq!(parse_chat_body(body).unwrap(), "done");
    }
}

#[cfg(test)]
mod http_tests {
    use super::{OpenAiChatClient, OpenAiChatConfig};
    use crate::message::message;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    #[tokio::test]
    async fn retries_on_429_then_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            // First attempt: 429 with Retry-After: 0 (instant retry).
            let (mut first, _) = listener.accept().expect("accept 1");
            drain_request(&mut first);
            let body = r#"{"error":{"message":"rate limited"}}"#;
            write!(
                first,
                "HTTP/1.1 429 Too Many Requests\r\nretry-after: 0\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            drop(first);
            // Second attempt: 200 with a normal completion.
            let (mut second, _) = listener.accept().expect("accept 2");
            drain_request(&mut second);
            let body = r#"{"choices":[{"message":{"content":"recovered"}}]}"#;
            write!(
                second,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let client = OpenAiChatClient::new(OpenAiChatConfig {
            api_key: "k".to_owned(),
            base_url,
            model: "m".to_owned(),
            temperature: None,
        })
        .unwrap();
        let out = client.chat(vec![message("user", "hi")]).await.unwrap();
        assert_eq!(
            out, "recovered",
            "client should retry the 429 and return the 200 body"
        );
        server.join().unwrap();
    }

    fn drain_request(stream: &mut TcpStream) {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let read = stream.read(&mut chunk).expect("read request");
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&buffer[..end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if buffer.len() >= end + 4 + content_length {
                    break;
                }
            }
        }
    }
}
