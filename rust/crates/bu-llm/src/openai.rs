//! OpenAI-shaped request/response bodies (chat-completions and responses).
//!
//! Transport, retries, and route fallback live in [`crate::client`]; this module
//! only knows what an OpenAI request looks like on the wire and how to read the
//! assistant text back out.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::LlmConfig;
use crate::message::ChatMessage;
use crate::responses::ResponsesRequest;

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

/// Builds a `POST {base}/chat/completions` body.
pub(crate) fn build_chat_request(
    config: &LlmConfig,
    messages: Vec<ChatMessage>,
) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: config.model.clone(),
        messages,
        temperature: config.temperature,
    }
}

/// Builds a `POST {base}/responses` body.
pub(crate) fn build_responses_request(
    config: &LlmConfig,
    messages: Vec<ChatMessage>,
) -> ResponsesRequest {
    ResponsesRequest::new(config.model.clone(), messages, config.temperature)
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
    // "length" means the model hit its output cap mid-token, so `content` is a
    // chopped prefix rather than an answer.
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    // Spec-valid responses (reasoning models, refusals, content filters, tool
    // calls) can send `content: null` or omit it; coerce to empty like Python's
    // `choice.message.content or ''` instead of failing to parse.
    #[serde(default)]
    content: Option<String>,
}

/// Extracts the assistant text from a successful chat-completions body. A
/// `null`, missing, or empty `content` yields an empty string (matching Python's
/// `content or ''`); only a genuinely empty `choices` array is an error.
pub(crate) fn parse_chat_body(body: &str) -> Result<String> {
    let parsed: ChatCompletionResponse =
        serde_json::from_str(body).context("failed to parse LLM chat response")?;
    let choice = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("LLM chat response contained no choices"))?;
    let text = choice.message.content.unwrap_or_default();
    if choice.finish_reason.as_deref() == Some("length") && text.is_empty() {
        return Err(anyhow!(
            "LLM response was cut off by the output limit before producing any content"
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmApi;
    use crate::message::message;
    use serde_json::json;

    fn config(model: &str) -> LlmConfig {
        LlmConfig {
            api_key: "k".to_owned(),
            base_url: "https://gw.example/v1".to_owned(),
            model: model.to_owned(),
            api: LlmApi::OpenAiChat,
            temperature: Some(0.7),
            max_tokens: 4096,
        }
    }

    #[test]
    fn chat_request_carries_model_messages_and_temperature() {
        let request = build_chat_request(&config("m"), vec![message("user", "hi")]);
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["model"], "m");
        // f32 -> JSON widens to 0.699999988079071, so compare with tolerance
        // rather than against the literal.
        let temperature = value["temperature"].as_f64().expect("temperature is a number");
        assert!(
            (temperature - 0.7).abs() < 1e-6,
            "temperature should round-trip as ~0.7, got {temperature}"
        );
        assert_eq!(value["messages"][0], json!({"role": "user", "content": "hi"}));
    }

    #[test]
    fn temperature_is_omitted_when_unset() {
        let mut config = config("m");
        config.temperature = None;
        let value = serde_json::to_value(build_chat_request(&config, vec![])).unwrap();
        assert!(value.get("temperature").is_none());
    }

    #[test]
    fn chat_body_reads_the_first_choice() {
        let body = r#"{"choices":[{"message":{"content":"hello"}}]}"#;
        assert_eq!(parse_chat_body(body).unwrap(), "hello");
    }

    #[test]
    fn null_content_becomes_empty_string() {
        let body = r#"{"choices":[{"message":{"content":null}}]}"#;
        assert_eq!(parse_chat_body(body).unwrap(), "");
    }

    #[test]
    fn no_choices_is_an_error() {
        assert!(parse_chat_body(r#"{"choices":[]}"#).is_err());
    }

    #[test]
    fn truncated_empty_response_names_the_output_limit() {
        let body = r#"{"choices":[{"message":{"content":""},"finish_reason":"length"}]}"#;
        let error = parse_chat_body(body).unwrap_err().to_string();
        assert!(error.contains("output limit"), "got: {error}");
    }
}
