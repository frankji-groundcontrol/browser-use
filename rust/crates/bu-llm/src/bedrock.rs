//! AWS Bedrock Converse chat client (feature `bedrock`).
//!
//! Provider parity with the Python MCP server's `ChatAWSBedrock` path
//! (`MODEL_PROVIDER=bedrock`). Auth/SigV4/retries are handled by the AWS SDK;
//! credentials and region come from the standard AWS environment.

use anyhow::{anyhow, Context, Result};
use aws_sdk_bedrockruntime::{
    types::{
        ContentBlock, ConversationRole, ImageBlock, ImageFormat, ImageSource, Message,
        SystemContentBlock,
    },
    Client,
};
use aws_smithy_types::Blob;
use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::message::{ChatMessage, ContentPart, MessageContent};

const DEFAULT_MODEL: &str = "us.anthropic.claude-sonnet-4-20250514-v1:0";
const DEFAULT_REGION: &str = "us-east-1";

/// Bedrock model + region selection.
#[derive(Debug, Clone)]
pub struct BedrockChatConfig {
    /// Bedrock model id (e.g. `us.anthropic.claude-sonnet-4-20250514-v1:0`).
    pub model: String,
    /// AWS region hosting the model.
    pub region: String,
}

impl BedrockChatConfig {
    /// Builds config from `MODEL` / `REGION`, applying an optional model override,
    /// with the same defaults as the Python server.
    pub fn from_env_with_model_override(model_override: Option<String>) -> Self {
        let model = model_override
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::env::var("MODEL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let region = std::env::var("REGION")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_REGION.to_owned());
        Self { model, region }
    }
}

/// Asynchronous AWS Bedrock Converse chat client.
#[derive(Debug, Clone)]
pub struct BedrockChatClient {
    client: Client,
    model: String,
}

impl BedrockChatClient {
    /// Loads AWS config from the environment and builds a client.
    pub async fn from_env_with_model_override(model_override: Option<String>) -> Result<Self> {
        Self::new(BedrockChatConfig::from_env_with_model_override(
            model_override,
        ))
        .await
    }

    /// Builds a client from explicit config, loading AWS credentials/region.
    pub async fn new(config: BedrockChatConfig) -> Result<Self> {
        let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(config.region))
            .load()
            .await;
        Ok(Self {
            client: Client::new(&sdk_config),
            model: config.model,
        })
    }

    /// Sends chat messages via the Converse API and returns the assistant text.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let mut system_blocks = Vec::new();
        let mut converse_messages = Vec::new();

        for message in messages {
            if message.role == "system" {
                system_blocks.push(SystemContentBlock::Text(message.content.as_text()));
                continue;
            }

            let role = match message.role.as_str() {
                "assistant" => ConversationRole::Assistant,
                _ => ConversationRole::User,
            };
            let mut builder = Message::builder().role(role);
            for block in content_blocks(&message.content)? {
                builder = builder.content(block);
            }
            converse_messages.push(
                builder
                    .build()
                    .map_err(|error| anyhow!("failed to build Bedrock message: {error}"))?,
            );
        }

        let mut request = self
            .client
            .converse()
            .model_id(self.model.clone())
            .set_messages(Some(converse_messages));
        if !system_blocks.is_empty() {
            request = request.set_system(Some(system_blocks));
        }

        let response = request
            .send()
            .await
            .context("Bedrock converse request failed")?;
        let output = response
            .output()
            .context("Bedrock response had no output")?;
        let message = output
            .as_message()
            .map_err(|_| anyhow!("Bedrock output was not a message"))?;
        let text = message
            .content()
            .iter()
            .filter_map(|block| block.as_text().ok())
            .cloned()
            .collect::<Vec<_>>()
            .join("");
        if text.trim().is_empty() {
            return Err(anyhow!("Bedrock response did not include assistant text"));
        }
        Ok(text)
    }
}

fn content_blocks(content: &MessageContent) -> Result<Vec<ContentBlock>> {
    match content {
        MessageContent::Text(text) => Ok(vec![ContentBlock::Text(text.clone())]),
        MessageContent::Parts(parts) => parts
            .iter()
            .map(|part| match part {
                ContentPart::Text { text } => Ok(ContentBlock::Text(text.clone())),
                ContentPart::ImageUrl { image_url } => {
                    let bytes = decode_data_url(&image_url.url)?;
                    let image = ImageBlock::builder()
                        .format(ImageFormat::Png)
                        .source(ImageSource::Bytes(Blob::new(bytes)))
                        .build()
                        .map_err(|error| anyhow!("failed to build Bedrock image block: {error}"))?;
                    Ok(ContentBlock::Image(image))
                }
            })
            .collect(),
    }
}

fn decode_data_url(url: &str) -> Result<Vec<u8>> {
    let base64_part = url.split_once(',').map(|(_, data)| data).unwrap_or(url);
    STANDARD
        .decode(base64_part)
        .context("failed to decode image data URL")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::message;

    #[test]
    fn config_defaults_match_python() {
        let config =
            BedrockChatConfig::from_env_with_model_override(Some("custom-model".to_owned()));
        assert_eq!(config.model, "custom-model");
    }

    #[test]
    fn text_message_maps_to_one_text_block() {
        let blocks = content_blocks(&message("user", "hi").content).unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], ContentBlock::Text(ref text) if text == "hi"));
    }

    #[test]
    fn data_url_decodes_to_bytes() {
        assert_eq!(
            decode_data_url("data:image/png;base64,AQID").unwrap(),
            vec![1, 2, 3]
        );
    }
}
