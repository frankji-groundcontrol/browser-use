//! HTTP LLM client: one retry/fallback loop, three wire formats.
//!
//! Transport concerns (timeouts, transient-failure backoff, wrong-route
//! recovery) are identical across protocols, so they live here once. Only body
//! construction, authentication, and response parsing vary by [`LlmApi`].

use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use crate::anthropic::{build_request as build_anthropic, parse_messages_body, ANTHROPIC_VERSION};
use crate::completion::{self, Completion, CompletionRequest};
use crate::config::{alternate_api_root, LlmApi, LlmConfig};
use crate::message::ChatMessage;
use crate::openai::{build_chat_request, build_responses_request, parse_chat_body};
use crate::responses::parse_responses_body;
use crate::stream::{SseDecoder, StreamEvent, StreamState};

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
                Some(base) => format!("{}/{}", base.trim_end_matches('/'), self.config.api.path()),
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

    /// Sends a typed completion request, retaining tool calls and token usage.
    pub async fn complete(&self, request: CompletionRequest) -> Result<Completion> {
        let body = completion::build_request(&self.config, request)?;
        let text = self.send_json(body, false).await?;
        completion::parse_completion(self.config.api, &text)
    }

    /// Streams a completion's text, tool arguments, and usage via server-sent events.
    pub async fn stream(
        &self,
        request: CompletionRequest,
        mut emit: impl FnMut(StreamEvent),
    ) -> Result<Completion> {
        let mut body = completion::build_request(&self.config, request)?;
        body["stream"] = serde_json::Value::Bool(true);
        let mut url = self.config.endpoint_url();
        let mut tried_fallback = false;
        let response = {
            let mut attempt = 0;
            loop {
                let result = self
                    .authenticate(self.http.post(&url))
                    .json(&body)
                    .send()
                    .await;
                match result {
                    Ok(response) if response.status().as_u16() == 404 && !tried_fallback => {
                        if let Some(fallback) = self.config.fallback_url() {
                            url = fallback;
                            tried_fallback = true;
                            continue;
                        }
                        break response;
                    }
                    Ok(response)
                        if (response.status().as_u16() == 429
                            || response.status().is_server_error())
                            && attempt < MAX_RETRIES =>
                    {
                        let retry_after = response
                            .headers()
                            .get(reqwest::header::RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse().ok());
                        Self::backoff_sleep(attempt, retry_after).await;
                        attempt += 1;
                    }
                    Ok(response) => break response,
                    Err(error)
                        if attempt < MAX_RETRIES && (error.is_timeout() || error.is_connect()) =>
                    {
                        Self::backoff_sleep(attempt, None).await;
                        attempt += 1;
                    }
                    Err(error) => {
                        return Err(anyhow::Error::new(error).context("LLM stream request failed"))
                    }
                }
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "LLM stream request failed with HTTP {status}: {body}"
            ));
        }
        let mut response = response;
        let mut decoder = SseDecoder::default();
        let mut state = StreamState::new(self.config.api);
        while let Some(chunk) = response
            .chunk()
            .await
            .context("failed reading LLM stream")?
        {
            for event in decoder.push(&chunk)? {
                state.event(&event, &mut emit)?;
                if state.is_done() {
                    break;
                }
            }
            if state.is_done() {
                break;
            }
        }
        state.finish()
    }

    async fn send_json(&self, body: serde_json::Value, _stream: bool) -> Result<String> {
        let mut url = self.config.endpoint_url();
        let mut tried_fallback = false;
        for attempt in 0..=MAX_RETRIES {
            let response = self
                .authenticate(self.http.post(&url))
                .json(&body)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error)
                    if attempt < MAX_RETRIES && (error.is_timeout() || error.is_connect()) =>
                {
                    Self::backoff_sleep(attempt, None).await;
                    continue;
                }
                Err(error) => {
                    return Err(anyhow::Error::new(error).context("LLM completion request failed"))
                }
            };
            if response.status().is_success() {
                return response
                    .text()
                    .await
                    .context("failed to read LLM response body");
            }
            let status = response.status();
            if status.as_u16() == 404 && !tried_fallback {
                if let Some(fallback) = self.config.fallback_url() {
                    url = fallback;
                    tried_fallback = true;
                    continue;
                }
            }
            if (status.as_u16() == 429 || status.is_server_error()) && attempt < MAX_RETRIES {
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse().ok());
                Self::backoff_sleep(attempt, retry_after).await;
                continue;
            }
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "LLM completion request failed with HTTP {status}: {body}"
            ));
        }
        Err(anyhow!("LLM completion request exhausted retries"))
    }

    fn build_body(&self, messages: Vec<ChatMessage>) -> Result<serde_json::Value> {
        let value = match self.config.api {
            LlmApi::OpenAiResponses => {
                serde_json::to_value(build_responses_request(&self.config, messages))
            }
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
#[path = "client_tests.rs"]
mod tests;
