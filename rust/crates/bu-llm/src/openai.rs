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
}
