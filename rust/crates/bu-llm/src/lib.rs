//! LLM clients for the Rust browser-use rewrite.
//!
//! [`LlmProvider`] is the provider-agnostic entry point used by the agent loop.
//! It dispatches to an OpenAI-compatible client or (behind the `bedrock` feature)
//! an AWS Bedrock client, mirroring the Python MCP server's provider selection.

mod message;
mod openai;

#[cfg(feature = "bedrock")]
mod bedrock;

pub use message::{message, message_with_image, ChatMessage, ContentPart, ImageUrl, MessageContent};
pub use openai::{OpenAiChatClient, OpenAiChatConfig};

#[cfg(feature = "bedrock")]
pub use bedrock::{BedrockChatClient, BedrockChatConfig};

/// Provider-agnostic chat backend selected at MCP-tool time.
#[derive(Debug, Clone)]
pub enum LlmProvider {
    /// OpenAI-compatible chat completions (OpenAI, Azure, gateways).
    OpenAi(OpenAiChatClient),
    /// AWS Bedrock Converse API.
    #[cfg(feature = "bedrock")]
    Bedrock(BedrockChatClient),
}

impl LlmProvider {
    /// Sends chat messages and returns the assistant text, regardless of provider.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> anyhow::Result<String> {
        match self {
            Self::OpenAi(client) => client.chat(messages).await,
            #[cfg(feature = "bedrock")]
            Self::Bedrock(client) => client.chat(messages).await,
        }
    }

    /// Human-readable provider + model label for logs and reports.
    pub fn label(&self) -> String {
        match self {
            Self::OpenAi(_) => "openai".to_owned(),
            #[cfg(feature = "bedrock")]
            Self::Bedrock(_) => "bedrock".to_owned(),
        }
    }
}

impl From<OpenAiChatClient> for LlmProvider {
    fn from(client: OpenAiChatClient) -> Self {
        Self::OpenAi(client)
    }
}
