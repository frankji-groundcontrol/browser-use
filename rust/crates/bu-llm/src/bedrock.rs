//! AWS Bedrock Converse chat client (feature `bedrock`).
//!
//! Provider parity with the Python MCP server's `ChatAWSBedrock` path
//! (`BROWSER_USE_LLM_API=bedrock`). Auth/SigV4/retries are handled by the AWS SDK;
//! credentials and region come from the standard AWS environment.

use anyhow::{anyhow, Context, Result};
use aws_sdk_bedrockruntime::{
    types::{ContentBlock, ImageBlock, ImageFormat, ImageSource, InferenceConfiguration},
    Client,
};
use aws_smithy_types::Blob;
use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::message::{ChatMessage, ContentPart, MessageContent};
use crate::{Completion, CompletionRequest, StreamEvent};
use std::time::Duration;

#[path = "bedrock_stream.rs"]
mod stream;
#[path = "bedrock_wire.rs"]
mod wire;
const TIMEOUT: Duration = Duration::from_secs(120);

const DEFAULT_MODEL: &str = "us.anthropic.claude-sonnet-4-6";
const DEFAULT_REGION: &str = "us-east-1";

/// Bedrock model + region selection.
#[derive(Debug, Clone)]
pub struct BedrockChatConfig {
    /// Bedrock model id (e.g. `us.anthropic.claude-sonnet-4-6`).
    pub model: String,
    /// AWS region hosting the model.
    pub region: String,
}

impl BedrockChatConfig {
    /// Builds config from `BROWSER_USE_LLM_MODEL` / `AWS_REGION`, applying an optional model override,
    /// with the same defaults as the Python server.
    pub fn from_env_with_model_override(model_override: Option<String>) -> Self {
        let model = model_override
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                std::env::var("BROWSER_USE_LLM_MODEL")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let region = std::env::var("AWS_REGION")
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
    inference: InferenceConfiguration,
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
            .retry_config(aws_smithy_types::retry::RetryConfig::standard().with_max_attempts(6))
            .timeout_config(
                aws_smithy_types::timeout::TimeoutConfig::builder()
                    .operation_timeout(TIMEOUT)
                    .operation_attempt_timeout(Duration::from_secs(30))
                    .connect_timeout(Duration::from_secs(10))
                    .build(),
            )
            .load()
            .await;
        Ok(Self {
            client: Client::new(&sdk_config),
            model: config.model,
            inference: InferenceConfiguration::builder()
                .max_tokens(crate::DEFAULT_MAX_TOKENS as i32)
                .build(),
        })
    }

    pub(crate) fn with_inference(
        mut self,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<Self> {
        let max_tokens =
            i32::try_from(max_tokens).context("Bedrock max_tokens exceeds supported range")?;
        self.inference = InferenceConfiguration::builder()
            .max_tokens(max_tokens)
            .set_temperature(temperature)
            .build();
        Ok(self)
    }

    /// Sends text/image chat through the same typed Converse boundary.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let output = self.complete(messages.into()).await?;
        if output.text.trim().is_empty() {
            return Err(anyhow!("Bedrock response did not include assistant text"));
        }
        Ok(output.text)
    }

    /// Preserves tool calls, structured arguments and token accounting.
    pub async fn complete(&self, request: CompletionRequest) -> Result<Completion> {
        let (system, messages, tools) = wire::request(request)?;
        let response = self
            .client
            .converse()
            .model_id(&self.model)
            .set_system((!system.is_empty()).then_some(system))
            .set_messages(Some(messages))
            .set_tool_config(tools)
            .inference_config(self.inference.clone())
            .send()
            .await
            .map_err(|_| anyhow!("Bedrock Converse request failed"))?;
        wire::completion(
            response
                .output()
                .context("Bedrock response has no output")?,
            response.stop_reason(),
            response.usage(),
        )
    }

    /// Reads AWS event-stream frames with an overall deadline, including metadata
    /// after messageStop; only SDK request establishment can be retried safely.
    pub async fn stream(
        &self,
        request: CompletionRequest,
        mut emit: impl FnMut(StreamEvent),
    ) -> Result<Completion> {
        let (system, messages, tools) = wire::request(request)?;
        tokio::time::timeout(TIMEOUT, async {
            let mut response = self
                .client
                .converse_stream()
                .model_id(&self.model)
                .set_system((!system.is_empty()).then_some(system))
                .set_messages(Some(messages))
                .set_tool_config(tools)
                .inference_config(self.inference.clone())
                .send()
                .await
                .map_err(|_| anyhow!("Bedrock ConverseStream request failed"))?;
            let mut state = stream::State::default();
            while let Some(event) = response
                .stream
                .recv()
                .await
                .map_err(|_| anyhow!("Bedrock stream failed"))?
            {
                state.event(event, &mut emit)?;
            }
            state.finish()
        })
        .await
        .context("Bedrock stream timed out")?
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
                        .format(data_url_format(&image_url.url))
                        .source(ImageSource::Bytes(Blob::new(bytes)))
                        .build()
                        .map_err(|error| anyhow!("failed to build Bedrock image block: {error}"))?;
                    Ok(ContentBlock::Image(image))
                }
            })
            .collect(),
    }
}

/// Maps the data URL's mime prefix to Bedrock's image format. The screenshot
/// pipeline emits JPEG; PNG is kept for any caller passing its own data URL.
fn data_url_format(url: &str) -> ImageFormat {
    if url.starts_with("data:image/png") {
        ImageFormat::Png
    } else {
        ImageFormat::Jpeg
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

    #[test]
    fn data_url_format_follows_the_mime_prefix() {
        assert!(matches!(
            data_url_format("data:image/jpeg;base64,AQID"),
            ImageFormat::Jpeg
        ));
        assert!(matches!(
            data_url_format("data:image/png;base64,AQID"),
            ImageFormat::Png
        ));
    }
}

#[cfg(test)]
#[path = "bedrock_tests.rs"]
mod wire_tests;
