//! OpenAI-compatible chat client for the Rust browser-use rewrite.

use std::{env, time::Duration};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-5.4-mini";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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
    content: String,
}

impl OpenAiChatConfig {
    /// Loads OpenAI-compatible chat configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        let api_key = env::var("OPENAI_API_KEY")
            .map(|value| value.trim().to_owned())
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("OPENAI_API_KEY is not set"))?;
        let base_url = env::var("OPENAI_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let model = env::var("BROWSER_USE_LLM_MODEL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let temperature = match env::var("BROWSER_USE_LLM_TEMPERATURE") {
            Ok(value) if !value.trim().is_empty() => Some(
                value
                    .parse::<f32>()
                    .with_context(|| format!("invalid BROWSER_USE_LLM_TEMPERATURE={value:?}"))?,
            ),
            _ => None,
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

    /// Creates a chat client from explicit configuration.
    pub fn new(config: OpenAiChatConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { http, config })
    }

    /// Sends a two-message extraction prompt and returns assistant message text.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let request = ChatCompletionRequest {
            model: self.config.model.clone(),
            messages,
            temperature: self.config.temperature,
        };

        let response = self
            .http
            .post(self.config.chat_completions_url())
            .bearer_auth(&self.config.api_key)
            .json(&request)
            .send()
            .await
            .context("LLM chat request failed")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read LLM response body")?;
        if !status.is_success() {
            return Err(anyhow!(
                "LLM chat request failed with HTTP {status}: {body}"
            ));
        }

        let parsed: ChatCompletionResponse =
            serde_json::from_str(&body).context("failed to parse LLM chat response")?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| anyhow!("LLM chat response did not include assistant content"))
    }
}

/// Convenience constructor for chat messages.
pub fn message(role: impl Into<String>, content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: content.into(),
    }
}
