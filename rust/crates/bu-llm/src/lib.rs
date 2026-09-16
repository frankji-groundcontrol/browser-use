//! LLM clients for the Rust browser-use rewrite.
//!
//! Configuration is one explicit set of `BROWSER_USE_LLM_*` variables: a base
//! URL, a key, a model, and a wire format ([`LlmApi`]). Credential presence
//! never selects a backend — only `BROWSER_USE_LLM_API` does.
//!
//! [`LlmProvider`] is the provider-agnostic entry point used by the agent loop.
//! It dispatches to [`LlmClient`] for the HTTP protocols (OpenAI responses,
//! OpenAI chat-completions, Anthropic messages) or, behind the `bedrock`
//! feature, to an AWS Bedrock client.

mod anthropic;
mod client;
mod completion;
mod config;
mod message;
mod openai;
mod responses;
mod stream;

#[cfg(feature = "bedrock")]
mod bedrock;

pub use anthropic::ANTHROPIC_VERSION;
pub use client::LlmClient;
pub use completion::{
    Completion, CompletionRequest, ConversationMessage, TokenUsage, ToolCall, ToolDefinition,
};
pub use config::{LlmApi, LlmConfig, DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE};
pub use message::{
    message, message_with_image, ChatMessage, ContentPart, ImageUrl, MessageContent,
};
pub use stream::StreamEvent;
#[cfg(test)]
#[path = "stream_tests.rs"]
mod stream_tests;

#[cfg(feature = "bedrock")]
pub use bedrock::{BedrockChatClient, BedrockChatConfig};

/// Provider-agnostic chat backend selected at MCP-tool time.
#[derive(Debug, Clone)]
pub enum LlmProvider {
    /// An HTTP LLM API (OpenAI responses/chat, or Anthropic messages).
    Http(LlmClient),
    /// AWS Bedrock Converse API.
    #[cfg(feature = "bedrock")]
    Bedrock(BedrockChatClient),
}

impl LlmProvider {
    /// Selects the backend from the explicit API setting for both CLI and MCP.
    pub async fn from_config(config: LlmConfig) -> anyhow::Result<Self> {
        #[cfg(feature = "bedrock")]
        if config.api == LlmApi::Bedrock {
            let client = BedrockChatClient::from_env_with_model_override(Some(config.model))
                .await?
                .with_inference(config.max_tokens, config.temperature)?;
            return Ok(Self::Bedrock(client));
        }
        Ok(Self::Http(LlmClient::new(config)?))
    }

    /// Sends chat messages and returns the assistant text, regardless of provider.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> anyhow::Result<String> {
        match self {
            Self::Http(client) => client.chat(messages).await,
            #[cfg(feature = "bedrock")]
            Self::Bedrock(client) => client.chat(messages).await,
        }
    }

    /// Sends a typed request and preserves tool calls and token usage.
    pub async fn complete(&self, request: CompletionRequest) -> anyhow::Result<Completion> {
        match self {
            Self::Http(client) => client.complete(request).await,
            #[cfg(feature = "bedrock")]
            Self::Bedrock(client) => client.complete(request).await,
        }
    }

    /// Streams provider events as they arrive, including tools and final usage.
    pub async fn stream(
        &self,
        request: CompletionRequest,
        emit: impl FnMut(StreamEvent),
    ) -> anyhow::Result<Completion> {
        match self {
            Self::Http(client) => client.stream(request, emit).await,
            #[cfg(feature = "bedrock")]
            Self::Bedrock(client) => client.stream(request, emit).await,
        }
    }

    /// Human-readable provider + model label for logs and reports.
    pub fn label(&self) -> String {
        match self {
            Self::Http(client) => client.config().api.label().to_owned(),
            #[cfg(feature = "bedrock")]
            Self::Bedrock(_) => "bedrock".to_owned(),
        }
    }
}

impl From<LlmClient> for LlmProvider {
    fn from(client: LlmClient) -> Self {
        Self::Http(client)
    }
}
