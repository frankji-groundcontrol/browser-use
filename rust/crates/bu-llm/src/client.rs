//! HTTP LLM client: one retry/fallback loop, three wire formats.
//!
//! Transport concerns (timeouts, transient-failure backoff, wrong-route
//! recovery) are identical across protocols, so they live here once. Only body
//! construction, authentication, and response parsing vary by [`LlmApi`].

use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use crate::anthropic::{build_request as build_anthropic, parse_messages_body, ANTHROPIC_VERSION};
use crate::config::{alternate_api_root, LlmApi, LlmConfig};
use crate::message::ChatMessage;
use crate::openai::{build_chat_request, build_responses_request, parse_chat_body};
use crate::responses::parse_responses_body;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
/// Transient-failure retries, mirroring the OpenAI SDK's `max_retries=5`.
const MAX_RETRIES: usize = 5;

/// Asynchronous LLM client over an OpenAI or Anthropic HTTP API.
#[derive(Debug, Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    config: LlmConfig,
}

impl LlmClient {
    /// Creates a client from environment configuration.
    pub fn from_env() -> Result<Self> {
        Self::new(LlmConfig::from_env()?)
    }

    /// Creates a client from environment configuration with a model override.
    pub fn from_env_with_model_override(model_override: Option<String>) -> Result<Self> {
        Self::new(LlmConfig::from_env_with_model_override(model_override)?)
    }

    /// Creates a client from explicit configuration.
    pub fn new(config: LlmConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { http, config })
    }

    /// The configuration this client will use.
    pub fn config(&self) -> &LlmConfig {
        &self.config
    }

    /// Sends chat messages and returns the assistant text.
    ///
    /// Retries transient failures (HTTP 429/5xx, connect/timeout) with
    /// exponential backoff honoring `Retry-After`.
    ///
    /// The configured base URL is used **exactly as given** first. Only if that
    /// route is absent — HTTP 404, or a 200 whose body is an HTML landing page,
    /// which is how gateways answer a wrong root — is the alternate root tried
    /// once (bare gains `/v1`, versioned loses it).
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let body = self.build_body(messages)?;

        let mut base_override: Option<String> = None;
        let mut first_url: Option<String> = None;
        let mut tried_alternate_route = false;
        let mut attempt = 0;
        loop {
            let url = match &base_override {
                Some(base) => format!(
                    "{}/{}",
                    base.trim_end_matches('/'),
                    self.config.api.path()
                ),
                None => self.config.endpoint_url(),
            };
            first_url.get_or_insert_with(|| url.clone());

            let request = self.authenticate(self.http.post(&url)).json(&body);
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    if attempt < MAX_RETRIES && (error.is_timeout() || error.is_connect()) {
                        Self::backoff_sleep(attempt, None).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(anyhow::Error::new(error).context("LLM chat request failed"));
                }
            };

            let status = response.status();
            let html_body = status.is_success()
                && response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|content_type| {
                        content_type.to_ascii_lowercase().contains("text/html")
                    });

            if status.as_u16() == 404 || html_body {
                if !tried_alternate_route {
                    tried_alternate_route = true;
                    let current_base = base_override
                        .clone()
                        .unwrap_or_else(|| self.config.base_url.clone());
                    base_override = Some(alternate_api_root(&current_base));
                    attempt += 1;
                    continue;
                }
                let what = if html_body {
                    "an HTML page instead of JSON"
                } else {
                    "HTTP 404"
                };
                let first = first_url.clone().unwrap_or_else(|| url.clone());
                return Err(anyhow!(
                    "no {} route: {what} at both {first} and {url}; check BROWSER_USE_LLM_BASE_URL and BROWSER_USE_LLM_API",
                    self.config.api.label()
                ));
            }

            if status.is_success() {
                let text = response
                    .text()
                    .await
                    .context("failed to read LLM response body")?;
                return self.parse_body(&text);
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
            // A raw "HTTP 402" tells an operator nothing; when the endpoint has a
            // known meaning for the status, lead with the fix.
            return Err(match status_hint(&self.config.base_url, status.as_u16()) {
                Some(hint) => anyhow!("{hint} (HTTP {status}: {body})"),
                None => anyhow!("LLM chat request failed with HTTP {status}: {body}"),
            });
        }
    }

    fn build_body(&self, messages: Vec<ChatMessage>) -> Result<serde_json::Value> {
        let value = match self.config.api {
            LlmApi::OpenAiResponses => serde_json::to_value(build_responses_request(
                &self.config,
                messages,
            )),
            LlmApi::OpenAiChat => serde_json::to_value(build_chat_request(&self.config, messages)),
            LlmApi::AnthropicMessages => {
                serde_json::to_value(build_anthropic(&self.config, messages))
            }
            #[cfg(feature = "bedrock")]
            LlmApi::Bedrock => {
                return Err(anyhow!(
                    "bedrock is not driven over HTTP; use BedrockChatClient"
                ))
            }
        };
        value.context("failed to serialize LLM request")
    }

    /// Applies the protocol's authentication. Anthropic uses `x-api-key` plus a
    /// required dated version header; the OpenAI formats use bearer auth.
    fn authenticate(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.config.api {
            LlmApi::AnthropicMessages => request
                .header("x-api-key", &self.config.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION),
            _ => request.bearer_auth(&self.config.api_key),
        }
    }

    fn parse_body(&self, text: &str) -> Result<String> {
        match self.config.api {
            LlmApi::OpenAiResponses => parse_responses_body(text),
            LlmApi::OpenAiChat => parse_chat_body(text),
            LlmApi::AnthropicMessages => parse_messages_body(text),
            #[cfg(feature = "bedrock")]
            LlmApi::Bedrock => Err(anyhow!("bedrock responses are not parsed here")),
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

/// Actionable guidance for a failing status, keyed off the endpoint host.
///
/// Browser Use's cloud LLM answers 401 for a bad key and 402 when credits run
/// out; both are operator-fixable and neither is obvious from the status alone.
/// Other hosts keep the plain error, because 402 elsewhere means something else.
fn status_hint(base_url: &str, status: u16) -> Option<&'static str> {
    if !base_url.contains("browser-use.com") {
        return None;
    }
    match status {
        401 => Some(
            "BROWSER_USE_LLM_API_KEY is invalid or missing. Get a new key at https://cloud.browser-use.com/new-api-key",
        ),
        402 => Some(
            "Browser Use credits exhausted. Add more at https://cloud.browser-use.com/billing",
        ),
        _ => None,
    }
}

impl LlmApi {
    /// Human-readable protocol name, used in route-failure messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "OpenAI responses",
            Self::OpenAiChat => "OpenAI chat-completions",
            Self::AnthropicMessages => "Anthropic messages",
            #[cfg(feature = "bedrock")]
            Self::Bedrock => "Bedrock",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_use_host_gets_actionable_hints() {
        let base = "https://llm.api.browser-use.com/v1";
        assert!(status_hint(base, 401).unwrap().contains("BROWSER_USE_LLM_API_KEY"));
        assert!(status_hint(base, 402).unwrap().contains("credits"));
        assert!(status_hint(base, 500).is_none(), "5xx is not operator-fixable");
    }

    #[test]
    fn other_hosts_keep_the_plain_error() {
        assert!(status_hint("https://api.openai.com/v1", 401).is_none());
        assert!(status_hint("https://api.anthropic.com/v1", 402).is_none());
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use crate::message::{message, message_with_image};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc,
        thread,
    };

    struct Reply {
        status: &'static str,
        content_type: &'static str,
        body: &'static str,
    }

    fn json_ok(body: &'static str) -> Reply {
        Reply {
            status: "200 OK",
            content_type: "application/json",
            body,
        }
    }

    /// Serves `replies` in order, returning the base URL and the request lines
    /// (request-line + headers + body) each attempt actually sent.
    fn serve(replies: Vec<Reply>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let raw = drain_request(&mut stream);
                let _ = tx.send(String::from_utf8_lossy(&raw).into_owned());
                let _ = write!(
                    stream,
                    "HTTP/1.1 {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    reply.status,
                    reply.content_type,
                    reply.body.len(),
                    reply.body
                );
            }
        });
        (base_url, rx)
    }

    fn client(base_url: String, api: LlmApi) -> LlmClient {
        LlmClient::new(LlmConfig {
            api_key: "secret-key".to_owned(),
            base_url,
            model: "m".to_owned(),
            api,
            temperature: None,
            max_tokens: 77,
        })
        .unwrap()
    }

    fn request_line(raw: &str) -> String {
        raw.lines().next().unwrap_or_default().to_owned()
    }

    #[tokio::test]
    async fn exact_url_is_used_first_without_any_v1_guessing() {
        // Bare host + chat: the exact route is {base}/chat/completions.
        let (base_url, requests) = serve(vec![json_ok(
            r#"{"choices":[{"message":{"content":"ok"}}]}"#,
        )]);
        let out = client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        assert_eq!(out, "ok");
        let first = request_line(&requests.recv().unwrap());
        assert!(
            first.contains("POST /chat/completions "),
            "exact URL must be tried first, got: {first}"
        );
        assert!(
            requests.try_recv().is_err(),
            "a working exact URL must not trigger a fallback attempt"
        );
    }

    #[tokio::test]
    async fn a_404_at_the_exact_url_falls_back_to_the_v1_root() {
        let (base_url, requests) = serve(vec![
            Reply {
                status: "404 Not Found",
                content_type: "application/json",
                body: r#"{"error":"no route"}"#,
            },
            json_ok(r#"{"choices":[{"message":{"content":"recovered"}}]}"#),
        ]);
        let out = client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        assert_eq!(out, "recovered");
        assert!(request_line(&requests.recv().unwrap()).contains("POST /chat/completions "));
        assert!(
            request_line(&requests.recv().unwrap()).contains("POST /v1/chat/completions "),
            "fallback should append /v1"
        );
    }

    #[tokio::test]
    async fn an_html_landing_page_also_triggers_the_fallback() {
        let (base_url, requests) = serve(vec![
            Reply {
                status: "200 OK",
                content_type: "text/html; charset=utf-8",
                body: "<html><body>gateway</body></html>",
            },
            json_ok(r#"{"choices":[{"message":{"content":"recovered"}}]}"#),
        ]);
        let out = client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        assert_eq!(out, "recovered", "a 200 HTML page is a wrong route, not an answer");
        drop(requests);
    }

    #[tokio::test]
    async fn neither_root_working_names_both_urls_tried() {
        let (base_url, _requests) = serve(vec![
            Reply {
                status: "404 Not Found",
                content_type: "application/json",
                body: "{}",
            },
            Reply {
                status: "404 Not Found",
                content_type: "application/json",
                body: "{}",
            },
        ]);
        let error = client(base_url, LlmApi::AnthropicMessages)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("/messages"), "should name the route: {error}");
        assert!(
            error.contains("/v1/messages"),
            "should name both roots tried: {error}"
        );
        assert!(
            error.contains("BROWSER_USE_LLM_BASE_URL"),
            "should name the variable to fix: {error}"
        );
    }

    #[tokio::test]
    async fn retries_on_429_then_succeeds() {
        let (base_url, _requests) = serve(vec![
            Reply {
                status: "429 Too Many Requests",
                content_type: "application/json",
                body: r#"{"error":"slow down"}"#,
            },
            json_ok(r#"{"choices":[{"message":{"content":"recovered"}}]}"#),
        ]);
        let out = client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        assert_eq!(out, "recovered");
    }

    #[tokio::test]
    async fn anthropic_sends_its_own_auth_headers_and_body_shape() {
        let (base_url, requests) = serve(vec![json_ok(
            r#"{"content":[{"type":"text","text":"hi back"}]}"#,
        )]);
        let out = client(base_url, LlmApi::AnthropicMessages)
            .chat(vec![
                message("system", "be terse"),
                message_with_image("user", "look", b"\x89PNG\r\n\x1a\nrest"),
            ])
            .await
            .unwrap();
        assert_eq!(out, "hi back");

        let raw = requests.recv().unwrap();
        let lower = raw.to_ascii_lowercase();
        assert!(lower.contains("post /messages "), "wrong route: {raw}");
        assert!(
            lower.contains("x-api-key: secret-key"),
            "Anthropic authenticates with x-api-key, not bearer: {raw}"
        );
        assert!(
            lower.contains(&format!("anthropic-version: {ANTHROPIC_VERSION}")),
            "the dated version header is required: {raw}"
        );
        assert!(
            !lower.contains("authorization: bearer"),
            "must not also send OpenAI bearer auth: {raw}"
        );

        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default();
        let json: serde_json::Value = serde_json::from_str(body).expect("body is JSON");
        assert_eq!(json["system"], "be terse", "system must be hoisted");
        assert_eq!(json["max_tokens"], 77, "max_tokens is required");
        assert_eq!(json["messages"][0]["content"][1]["type"], "image");
        assert_eq!(json["messages"][0]["content"][1]["source"]["type"], "base64");
    }

    #[tokio::test]
    async fn openai_sends_bearer_auth() {
        let (base_url, requests) = serve(vec![json_ok(
            r#"{"choices":[{"message":{"content":"ok"}}]}"#,
        )]);
        client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        let lower = requests.recv().unwrap().to_ascii_lowercase();
        assert!(lower.contains("authorization: bearer secret-key"), "{lower}");
        assert!(!lower.contains("x-api-key"), "must not send Anthropic auth");
    }

    fn drain_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let Ok(read) = stream.read(&mut chunk) else {
                break;
            };
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
        buffer
    }
}
