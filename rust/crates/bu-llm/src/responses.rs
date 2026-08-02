//! OpenAI **Responses** API (`POST {base}/responses`).
//!
//! The default request shape for OpenAI-style endpoints. Chat Completions
//! remains available behind `BROWSER_USE_OPENAI_API=chat_completions` for
//! gateways that only implement the older route.
//!
//! Shapes differ from Chat Completions in three ways that matter here:
//! - input parts are typed `input_text` / `input_image` rather than
//!   `text` / `image_url`
//! - the answer is an `output` ARRAY, and reasoning models put a `reasoning`
//!   item ahead of the `message` item, so the text cannot be read positionally
//! - truncation shows up as `status: "incomplete"` with
//!   `incomplete_details.reason`, not as `finish_reason`

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::message::{ChatMessage, ContentPart, MessageContent};

/// Request body for `POST /responses`.
#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ResponsesRequest {
    pub(crate) model: String,
    pub(crate) input: Vec<InputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct InputItem {
    role: String,
    content: Vec<InputContent>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InputContent {
    InputText { text: String },
    InputImage { image_url: String },
}

impl ResponsesRequest {
    /// Converts the provider-agnostic message list into Responses input items.
    pub(crate) fn new(model: String, messages: Vec<ChatMessage>, temperature: Option<f32>) -> Self {
        let input = messages
            .into_iter()
            .map(|message| InputItem {
                role: message.role,
                content: match message.content {
                    MessageContent::Text(text) => vec![InputContent::InputText { text }],
                    MessageContent::Parts(parts) => parts
                        .into_iter()
                        .map(|part| match part {
                            ContentPart::Text { text } => InputContent::InputText { text },
                            ContentPart::ImageUrl { image_url } => InputContent::InputImage {
                                image_url: image_url.url,
                            },
                        })
                        .collect(),
                },
            })
            .collect();
        Self {
            model,
            input,
            temperature,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ResponsesBody {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    incomplete_details: Option<IncompleteDetails>,
    #[serde(default)]
    output: Vec<OutputItem>,
}

#[derive(Debug, Deserialize)]
struct IncompleteDetails {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OutputItem {
    #[serde(default, rename = "type")]
    item_type: Option<String>,
    #[serde(default)]
    content: Vec<OutputContent>,
}

#[derive(Debug, Deserialize)]
struct OutputContent {
    #[serde(default, rename = "type")]
    content_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

/// Extracts the assistant text from a successful `/responses` body.
pub(crate) fn parse_responses_body(body: &str) -> Result<String> {
    let parsed: ResponsesBody = serde_json::from_str(body)
        .map_err(|error| anyhow!("failed to parse LLM response: {error}"))?;

    // Truncation must not reach a caller: the agent JSON-parses this text and
    // extract_content shows it verbatim, so a chopped prefix is never an answer.
    // Mirrors the finish_reason=="length" guard on the Chat Completions path.
    if parsed.status.as_deref() == Some("incomplete") {
        let reason = parsed
            .incomplete_details
            .and_then(|details| details.reason)
            .unwrap_or_else(|| "unknown".to_owned());
        return Err(anyhow!(
            "model output was truncated ({reason}); the response is incomplete. Request shorter output or raise the server-side cap."
        ));
    }

    // Only `message` items carry the answer; reasoning models emit a `reasoning`
    // item first, whose content must not be spliced into the reply.
    let text = parsed
        .output
        .into_iter()
        .filter(|item| item.item_type.as_deref() == Some("message"))
        .flat_map(|item| item.content)
        .filter(|content| content.content_type.as_deref() == Some("output_text"))
        .filter_map(|content| content.text)
        .collect::<Vec<_>>()
        .join("");

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{message, message_with_image};

    #[test]
    fn text_and_image_messages_become_typed_input_parts() {
        let request = ResponsesRequest::new(
            "m".to_owned(),
            vec![
                message("system", "be brief"),
                message_with_image("user", "what is this", &[0xff, 0xd8, 0xff]),
            ],
            Some(0.5),
        );
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["input"][0]["role"], "system");
        assert_eq!(json["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(json["input"][0]["content"][0]["text"], "be brief");
        // Responses uses input_image with a bare image_url string, NOT chat
        // completions' nested {"image_url": {"url": ...}}.
        assert_eq!(json["input"][1]["content"][1]["type"], "input_image");
        assert!(json["input"][1]["content"][1]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/jpeg;base64,"));
        assert_eq!(json["temperature"], 0.5);
    }

    #[test]
    fn reads_output_text_from_the_message_item() {
        // Captured verbatim from a live gateway response.
        let body = r#"{"id":"msg_1","object":"response","model":"claude-sonnet-4-5","status":"completed",
            "output":[{"type":"message","id":"item_1","role":"assistant",
            "content":[{"type":"output_text","text":"HELLO"}],"status":"completed"}],
            "usage":{"input_tokens":370,"output_tokens":5,"total_tokens":375}}"#;
        assert_eq!(parse_responses_body(body).unwrap(), "HELLO");
    }

    #[test]
    fn skips_reasoning_items_and_joins_text_parts() {
        // A reasoning model puts its private trace first; splicing it into the
        // answer would corrupt the agent's JSON parse.
        let body = r#"{"status":"completed","output":[
            {"type":"reasoning","content":[{"type":"reasoning_text","text":"thinking..."}]},
            {"type":"message","content":[
                {"type":"output_text","text":"one "},
                {"type":"output_text","text":"two"}]}]}"#;
        assert_eq!(parse_responses_body(body).unwrap(), "one two");
    }

    #[test]
    fn incomplete_status_is_a_truncation_error() {
        let body = r#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},
            "output":[{"type":"message","content":[{"type":"output_text","text":"{\"action\":\"cli"}]}]}"#;
        let error = parse_responses_body(body)
            .expect_err("a truncated response must not be returned")
            .to_string();
        assert!(error.contains("truncated"), "got: {error}");
        assert!(error.contains("max_output_tokens"), "got: {error}");
    }

    #[test]
    fn an_empty_output_is_an_empty_string_not_an_error() {
        // Matches the Chat Completions path, where a null content coerces to "".
        assert_eq!(
            parse_responses_body(r#"{"status":"completed","output":[]}"#).unwrap(),
            ""
        );
    }
}
