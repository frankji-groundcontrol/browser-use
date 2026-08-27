//! Anthropic Messages API request/response bodies.
//!
//! Genuinely different from the OpenAI shapes, not a header swap:
//!
//! - `system` is a **top-level field**, not a message role, so system messages
//!   must be hoisted out of the array.
//! - `max_tokens` is **required**.
//! - Images are `{type:"image", source:{type:"base64", media_type, data}}`
//!   rather than an `image_url` carrying a data URL.
//! - Auth is `x-api-key` plus a required `anthropic-version` header.
//!
//! Before this module the "Anthropic" configuration sent OpenAI-shaped bodies to
//! an `ANTHROPIC_BASE_URL`, which only worked because the gateways in use served
//! both protocols. Pointed at `api.anthropic.com` it failed.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::LlmConfig;
use crate::message::{ChatMessage, ContentPart, MessageContent};

/// Anthropic's dated API version. Required on every request.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct MessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<RequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize, PartialEq)]
struct RequestMessage {
    role: String,
    content: Vec<RequestBlock>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RequestBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
}

#[derive(Debug, Serialize, PartialEq)]
struct ImageSource {
    #[serde(rename = "type")]
    kind: &'static str,
    media_type: String,
    data: String,
}

/// Builds a Messages request, hoisting system messages into the top-level
/// `system` field.
///
/// Several system messages are joined rather than dropped, and a non-system
/// message that follows one still keeps its order in `messages`.
pub(crate) fn build_request(config: &LlmConfig, messages: Vec<ChatMessage>) -> MessagesRequest {
    let mut system = Vec::new();
    let mut converted = Vec::new();

    for message in messages {
        if message.role == "system" {
            system.push(message.content.as_text());
            continue;
        }
        converted.push(RequestMessage {
            // Anthropic accepts only "user" and "assistant".
            role: if message.role == "assistant" {
                "assistant".to_owned()
            } else {
                "user".to_owned()
            },
            content: blocks_for(message.content),
        });
    }

    MessagesRequest {
        model: config.model.clone(),
        max_tokens: config.max_tokens,
        system: (!system.is_empty()).then(|| system.join("\n\n")),
        messages: converted,
        temperature: config.temperature,
    }
}

fn blocks_for(content: MessageContent) -> Vec<RequestBlock> {
    match content {
        MessageContent::Text(text) => vec![RequestBlock::Text { text }],
        MessageContent::Parts(parts) => parts
            .into_iter()
            .map(|part| match part {
                ContentPart::Text { text } => RequestBlock::Text { text },
                ContentPart::ImageUrl { image_url } => match split_data_url(&image_url.url) {
                    Some((media_type, data)) => RequestBlock::Image {
                        source: ImageSource {
                            kind: "base64",
                            media_type,
                            data,
                        },
                    },
                    // A remote URL cannot be forwarded: Anthropic takes inline
                    // base64 only. Degrade to text naming the URL rather than
                    // dropping content silently.
                    None => RequestBlock::Text {
                        text: format!("[image at {}]", image_url.url),
                    },
                },
            })
            .collect(),
    }
}

/// Splits `data:image/png;base64,AAAA` into its media type and payload.
fn split_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let media_type = meta.strip_suffix(";base64")?;
    Some((media_type.to_owned(), data.to_owned()))
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    content: Vec<ResponseBlock>,
}

#[derive(Debug, Deserialize)]
struct ResponseBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

/// Extracts assistant text from a Messages response, concatenating every text
/// block. Non-text blocks (tool use, thinking) are skipped rather than treated
/// as errors; an empty result is legitimate, matching how the OpenAI path
/// coerces a null `content` to an empty string.
pub(crate) fn parse_messages_body(body: &str) -> Result<String> {
    let parsed: MessagesResponse =
        serde_json::from_str(body).context("failed to parse Anthropic messages response")?;
    if parsed.content.is_empty() {
        return Err(anyhow!("Anthropic response contained no content blocks"));
    }
    Ok(parsed
        .content
        .into_iter()
        .filter(|block| block.kind == "text")
        .filter_map(|block| block.text)
        .collect::<Vec<_>>()
        .join(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmApi;
    use crate::message::{message, message_with_image};
    use serde_json::json;

    fn config() -> LlmConfig {
        LlmConfig {
            api_key: "k".to_owned(),
            base_url: "https://api.anthropic.com/v1".to_owned(),
            model: "claude-sonnet-4-6".to_owned(),
            api: LlmApi::AnthropicMessages,
            temperature: Some(0.5),
            max_tokens: 1234,
        }
    }

    #[test]
    fn system_messages_are_hoisted_out_of_the_array() {
        let request = build_request(
            &config(),
            vec![message("system", "be terse"), message("user", "hi")],
        );
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["system"], "be terse");
        assert_eq!(value["messages"].as_array().unwrap().len(), 1);
        assert_eq!(value["messages"][0]["role"], "user");
    }

    #[test]
    fn several_system_messages_are_joined_not_dropped() {
        let request = build_request(
            &config(),
            vec![
                message("system", "first"),
                message("system", "second"),
                message("user", "hi"),
            ],
        );
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["system"], "first\n\nsecond");
    }

    #[test]
    fn max_tokens_is_always_present_because_anthropic_requires_it() {
        let request = build_request(&config(), vec![message("user", "hi")]);
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["max_tokens"], 1234);
    }

    #[test]
    fn images_convert_to_inline_base64_source() {
        let request = build_request(
            &config(),
            vec![message_with_image("user", "look", b"\x89PNG\r\n\x1a\nrest")],
        );
        let value = serde_json::to_value(&request).unwrap();
        let blocks = &value["messages"][0]["content"];
        assert_eq!(blocks[0], json!({"type": "text", "text": "look"}));
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["type"], "base64");
        assert_eq!(blocks[1]["source"]["media_type"], "image/png");
        assert!(
            blocks[1]["source"]["data"].as_str().unwrap().len() > 4,
            "image payload should carry the encoded bytes"
        );
        assert!(
            blocks[1].get("image_url").is_none(),
            "must not leak the OpenAI image shape"
        );
    }

    #[test]
    fn unknown_roles_collapse_to_user() {
        let request = build_request(&config(), vec![message("tool", "result")]);
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["messages"][0]["role"], "user");
    }

    #[test]
    fn assistant_role_is_preserved() {
        let request = build_request(&config(), vec![message("assistant", "prior")]);
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["messages"][0]["role"], "assistant");
    }

    #[test]
    fn response_text_blocks_are_concatenated() {
        let body = r#"{"content":[{"type":"text","text":"one "},{"type":"text","text":"two"}]}"#;
        assert_eq!(parse_messages_body(body).unwrap(), "one two");
    }

    #[test]
    fn non_text_blocks_are_skipped() {
        let body = r#"{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"answer"}]}"#;
        assert_eq!(parse_messages_body(body).unwrap(), "answer");
    }

    #[test]
    fn empty_content_is_an_error() {
        assert!(parse_messages_body(r#"{"content":[]}"#).is_err());
    }
}
