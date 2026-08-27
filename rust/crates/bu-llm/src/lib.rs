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
mod config;
mod message;
mod openai;
mod responses;

#[cfg(feature = "bedrock")]
mod bedrock;

pub use anthropic::ANTHROPIC_VERSION;
pub use client::LlmClient;
pub use config::{LlmApi, LlmConfig, DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE};
pub use message::{
    message, message_with_image, ChatMessage, ContentPart, ImageUrl, MessageContent,
};

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
    /// Sends chat messages and returns the assistant text, regardless of provider.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> anyhow::Result<String> {
        match self {
            Self::Http(client) => client.chat(messages).await,
            #[cfg(feature = "bedrock")]
            Self::Bedrock(client) => client.chat(messages).await,
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
