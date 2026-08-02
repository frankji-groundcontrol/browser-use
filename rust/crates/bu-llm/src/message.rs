//! Chat message types with optional multimodal (text + image) content.
//!
//! `content` serializes as a bare string for text-only messages, or as an array
//! of typed parts when a screenshot is attached — matching the OpenAI chat
//! completions multimodal shape (also accepted by Bedrock's converter).

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

/// A single chat message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: MessageContent,
}

/// Message body: plain text, or an ordered list of parts (text + images).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    /// Text-only body (serializes as a bare JSON string).
    Text(String),
    /// Multimodal body (serializes as a JSON array of parts).
    Parts(Vec<ContentPart>),
}

/// One part of a multimodal message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    /// A run of text.
    Text { text: String },
    /// An inline image referenced by data URL.
    ImageUrl { image_url: ImageUrl },
}

/// Image reference (data URL or remote URL).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageUrl {
    pub url: String,
}

/// Returns the MIME type for known image magic bytes, defaulting to JPEG (the
/// screenshot pipeline's default encoding) for anything unrecognized.
fn sniff_image_mime(image: &[u8]) -> &'static str {
    if image.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else {
        "image/jpeg"
    }
}

impl MessageContent {
    /// Returns the concatenated text of the message, ignoring images.
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text } => Some(text.as_str()),
                    ContentPart::ImageUrl { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// Builds a text-only chat message.
pub fn message(role: impl Into<String>, content: impl Into<String>) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: MessageContent::Text(content.into()),
    }
}

/// Builds a chat message with a screenshot attached after the text as a base64
/// data URL. The MIME type is sniffed from the image magic bytes (JPEG or PNG),
/// so it cannot drift from whatever encoding the capture actually used.
pub fn message_with_image(
    role: impl Into<String>,
    text: impl Into<String>,
    image: &[u8],
) -> ChatMessage {
    let data_url = format!("data:{};base64,{}", sniff_image_mime(image), STANDARD.encode(image));
    ChatMessage {
        role: role.into(),
        content: MessageContent::Parts(vec![
            ContentPart::Text { text: text.into() },
            ContentPart::ImageUrl {
                image_url: ImageUrl { url: data_url },
            },
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_message_serializes_as_bare_string() {
        let value = serde_json::to_value(message("system", "hello")).unwrap();
        assert_eq!(value, json!({"role": "system", "content": "hello"}));
    }

    #[test]
    fn image_mime_is_sniffed_from_magic_bytes() {
        assert_eq!(sniff_image_mime(b"\x89PNG\r\n\x1a\nrest"), "image/png");
        assert_eq!(sniff_image_mime(b"\xff\xd8\xffrest"), "image/jpeg");
        // Unknown bytes fall back to the pipeline default.
        assert_eq!(sniff_image_mime(&[1, 2, 3]), "image/jpeg");
    }

    #[test]
    fn image_message_serializes_as_openai_parts() {
        let value = serde_json::to_value(message_with_image("user", "look", &[1, 2, 3])).unwrap();
        assert_eq!(value["role"], "user");
        assert_eq!(value["content"][0], json!({"type": "text", "text": "look"}));
        assert_eq!(value["content"][1]["type"], "image_url");
        assert_eq!(
            value["content"][1]["image_url"]["url"],
            "data:image/jpeg;base64,AQID"
        );
    }
}
