use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::{LlmApi, LlmConfig};
use crate::message::{ChatMessage, ContentPart, MessageContent};
use crate::responses;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Completion {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConversationMessage {
    Chat(ChatMessage),
    Assistant {
        text: String,
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompletionRequest {
    pub messages: Vec<ConversationMessage>,
    pub tools: Vec<ToolDefinition>,
}
impl From<Vec<ChatMessage>> for CompletionRequest {
    fn from(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages: messages
                .into_iter()
                .map(ConversationMessage::Chat)
                .collect(),
            tools: vec![],
        }
    }
}

fn validate_tool(name: &str, parameters: &Value) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(anyhow!("invalid tool name: {name:?}"));
    }
    if !parameters.is_object() {
        return Err(anyhow!("tool parameters must be a JSON object"));
    }
    Ok(())
}
fn validate(req: &CompletionRequest) -> Result<()> {
    for t in &req.tools {
        validate_tool(&t.name, &t.parameters)?;
    }
    for m in &req.messages {
        if let ConversationMessage::Assistant { tool_calls, .. } = m {
            for c in tool_calls {
                if c.id.is_empty() || c.name.is_empty() || !c.arguments.is_object() {
                    return Err(anyhow!("invalid tool call"));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn build_request(config: &LlmConfig, request: CompletionRequest) -> Result<Value> {
    validate(&request)?;
    match config.api {
        LlmApi::OpenAiChat => build_chat(config, request),
        LlmApi::OpenAiResponses => build_responses(config, request),
        LlmApi::AnthropicMessages => build_anthropic(config, request),
        #[cfg(feature = "bedrock")]
        LlmApi::Bedrock => Err(anyhow!("bedrock completions are not HTTP")),
    }
}

fn chat_content(c: &ChatMessage) -> Value {
    serde_json::to_value(c).unwrap_or_else(|_| json!(c.content.as_text()))
}
fn build_chat(config: &LlmConfig, req: CompletionRequest) -> Result<Value> {
    let mut messages = Vec::new();
    for m in req.messages {
        match m {
            ConversationMessage::Chat(c) => messages.push(chat_content(&c)),
            ConversationMessage::Assistant { text, tool_calls } => {
                let mut value = json!({"role":"assistant","content":text});
                if !tool_calls.is_empty() {
                    value["tool_calls"] = json!(tool_calls.iter().map(|c| json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments.to_string()}})).collect::<Vec<_>>());
                }
                messages.push(value);
            }
            ConversationMessage::Tool { call_id, content } => {
                messages.push(json!({"role":"tool","tool_call_id":call_id,"content":content}))
            }
        }
    }
    let mut out = json!({"model": config.model, "messages": messages});
    if let Some(t) = config.temperature {
        out["temperature"] = json!(t);
    }
    if !req.tools.is_empty() {
        out["tools"] = json!(req.tools.iter().map(|t| json!({"type":"function","function":{"name":t.name,"description":t.description,"parameters":t.parameters}})).collect::<Vec<_>>());
    }
    Ok(out)
}
fn response_input(m: ConversationMessage) -> Vec<Value> {
    match m {
        ConversationMessage::Chat(c) => vec![serde_json::to_value(
            responses::ResponsesRequest::new("".into(), vec![c], None).input,
        )
        .unwrap_or_default()[0]
            .clone()],
        ConversationMessage::Assistant { text, tool_calls } => {
            let mut out =
                vec![json!({"role":"assistant","content":[{"type":"input_text","text":text}]})];
            out.extend(tool_calls.into_iter().map(|c| json!({"type":"function_call","call_id":c.id,"name":c.name,"arguments":c.arguments.to_string()})));
            out
        }
        ConversationMessage::Tool { call_id, content } => {
            vec![json!({"type":"function_call_output","call_id":call_id,"output":content})]
        }
    }
}
fn build_responses(config: &LlmConfig, req: CompletionRequest) -> Result<Value> {
    let input = req
        .messages
        .into_iter()
        .flat_map(response_input)
        .collect::<Vec<_>>();
    let mut out = json!({"model": config.model, "input": input});
    if let Some(t) = config.temperature {
        out["temperature"] = json!(t);
    }
    if !req.tools.is_empty() {
        out["tools"] = json!(req.tools.iter().map(|t| json!({"type":"function","name":t.name,"description":t.description,"parameters":t.parameters})).collect::<Vec<_>>());
    }
    Ok(out)
}
fn build_anthropic(config: &LlmConfig, req: CompletionRequest) -> Result<Value> {
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for m in req.messages {
        match m {
            ConversationMessage::Chat(c) if c.role == "system" => system.push(c.content.as_text()),
            ConversationMessage::Chat(c) => messages.push(json!({"role": if c.role == "assistant" {"assistant"} else {"user"}, "content": anthropic_content(c.content)})),
            ConversationMessage::Assistant { text, tool_calls } => {
                let mut content = vec![json!({"type":"text","text":text})];
                content.extend(tool_calls.into_iter().map(|c| json!({"type":"tool_use","id":c.id,"name":c.name,"input":c.arguments})));
                messages.push(json!({"role":"assistant","content": content}));
            }
            ConversationMessage::Tool { call_id, content } => messages.push(json!({"role":"user","content":[{"type":"tool_result","tool_use_id":call_id,"content":content}]})),
        }
    }
    let mut out = json!({"model":config.model,"max_tokens":config.max_tokens,"messages":messages});
    if !system.is_empty() {
        out["system"] = json!(system.join("\n\n"));
    }
    if let Some(t) = config.temperature {
        out["temperature"] = json!(t);
    }
    if !req.tools.is_empty() {
        out["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({"name":t.name,"description":t.description,"input_schema":t.parameters}))
            .collect::<Vec<_>>());
    }
    Ok(out)
}
fn anthropic_content(c: MessageContent) -> Value {
    match c {
        MessageContent::Text(t) => json!(t),
        MessageContent::Parts(p) => Value::Array(
            p.into_iter()
                .map(|x| match x {
                    ContentPart::Text { text } => json!({"type":"text","text":text}),
                    ContentPart::ImageUrl { image_url } => image_block(&image_url.url),
                })
                .collect(),
        ),
    }
}

fn image_block(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((meta, data)) = rest.split_once(',') {
            if let Some(media_type) = meta.strip_suffix(";base64") {
                return json!({"type":"image","source":{"type":"base64","media_type":media_type,"data":data}});
            }
        }
    }
    // Anthropic accepts remote images as URL sources; preserve the modality
    // instead of degrading the request to a textual placeholder.
    json!({"type":"image","source":{"type":"url","url":url}})
}

pub(crate) fn parse_completion(api: LlmApi, body: &str) -> Result<Completion> {
    match api {
        LlmApi::OpenAiChat => parse_chat(body),
        LlmApi::OpenAiResponses => parse_resp(body),
        LlmApi::AnthropicMessages => parse_anthropic(body),
        #[cfg(feature = "bedrock")]
        LlmApi::Bedrock => Err(anyhow!("bedrock completion parser unavailable")),
    }
}
fn parse_chat(body: &str) -> Result<Completion> {
    let v: Value = serde_json::from_str(body).context("failed to parse chat response")?;
    let ch = v["choices"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow!("LLM chat response contained no choices"))?;
    let text = ch["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if ch["finish_reason"] == "length" {
        return Err(anyhow!("LLM response was truncated at the output limit"));
    }
    let mut calls = Vec::new();
    if let Some(arr) = ch["message"]["tool_calls"].as_array() {
        for c in arr {
            let a = c["function"]["arguments"]
                .as_str()
                .ok_or_else(|| anyhow!("tool call arguments are not a string"))?;
            let arguments: Value =
                serde_json::from_str(a).context("invalid tool call arguments")?;
            if !arguments.is_object() {
                return Err(anyhow!("tool call arguments must be an object"));
            }
            let id = c["id"].as_str().unwrap_or_default();
            let name = c["function"]["name"].as_str().unwrap_or_default();
            if id.is_empty() || name.is_empty() {
                return Err(anyhow!("tool call missing id or name"));
            }
            calls.push(ToolCall {
                id: id.into(),
                name: name.into(),
                arguments,
            });
        }
    }
    Ok(Completion {
        text,
        tool_calls: calls,
        usage: usage(&v["usage"], "prompt_tokens", "completion_tokens"),
        finish_reason: ch["finish_reason"].as_str().map(String::from),
    })
}
fn parse_resp(body: &str) -> Result<Completion> {
    let v: Value = serde_json::from_str(body).context("failed to parse responses body")?;
    let mut text = String::new();
    let mut calls = Vec::new();
    for i in v["output"].as_array().into_iter().flatten() {
        match i["type"].as_str() {
            Some("message") => {
                for c in i["content"].as_array().into_iter().flatten() {
                    if c["type"] == "output_text" {
                        text.push_str(c["text"].as_str().unwrap_or_default())
                    }
                }
            }
            Some("function_call") => {
                let a = i["arguments"]
                    .as_str()
                    .ok_or_else(|| anyhow!("function call arguments are not a string"))?;
                let arguments: Value =
                    serde_json::from_str(a).context("invalid function call arguments")?;
                if !arguments.is_object() {
                    return Err(anyhow!("tool call arguments must be an object"));
                }
                let id = i["call_id"]
                    .as_str()
                    .or_else(|| i["id"].as_str())
                    .unwrap_or_default();
                let name = i["name"].as_str().unwrap_or_default();
                if id.is_empty() || name.is_empty() {
                    return Err(anyhow!("tool call missing id or name"));
                }
                calls.push(ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments,
                });
            }
            _ => {}
        }
    }
    Ok(Completion {
        text,
        tool_calls: calls,
        usage: usage(&v["usage"], "input_tokens", "output_tokens"),
        finish_reason: v["status"].as_str().map(String::from),
    })
}
fn parse_anthropic(body: &str) -> Result<Completion> {
    let v: Value = serde_json::from_str(body).context("failed to parse Anthropic response")?;
    if v["stop_reason"] == "max_tokens" {
        return Err(anyhow!("Anthropic response was truncated at max_tokens"));
    }
    let mut text = String::new();
    let mut calls = Vec::new();
    for b in v["content"].as_array().into_iter().flatten() {
        match b["type"].as_str() {
            Some("text") => text.push_str(b["text"].as_str().unwrap_or_default()),
            Some("tool_use") => {
                let a = b["input"].clone();
                if !a.is_object() {
                    return Err(anyhow!("tool call arguments must be an object"));
                }
                let id = b["id"].as_str().unwrap_or_default();
                let name = b["name"].as_str().unwrap_or_default();
                if id.is_empty() || name.is_empty() {
                    return Err(anyhow!("tool call missing id or name"));
                }
                calls.push(ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments: a,
                });
            }
            _ => {}
        }
    }
    Ok(Completion {
        text,
        tool_calls: calls,
        usage: usage(&v["usage"], "input_tokens", "output_tokens"),
        finish_reason: v["stop_reason"].as_str().map(String::from),
    })
}
fn usage(v: &Value, inp: &str, out: &str) -> Option<TokenUsage> {
    let i = v[inp].as_u64()?;
    let o = v[out].as_u64()?;
    Some(TokenUsage {
        input_tokens: i,
        output_tokens: o,
        total_tokens: v["total_tokens"].as_u64().unwrap_or(i + o),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cfg(api: LlmApi) -> LlmConfig {
        LlmConfig {
            api_key: "k".into(),
            base_url: "https://example/v1".into(),
            model: "m".into(),
            api,
            temperature: None,
            max_tokens: 100,
        }
    }
    #[test]
    fn chat_tools_use_native_shape() {
        let req = CompletionRequest {
            messages: vec![ConversationMessage::Chat(ChatMessage {
                role: "user".into(),
                content: MessageContent::Text("hi".into()),
            })],
            tools: vec![ToolDefinition {
                name: "lookup".into(),
                description: "find".into(),
                parameters: json!({"type":"object"}),
            }],
        };
        let body = build_request(&cfg(LlmApi::OpenAiChat), req).unwrap();
        assert_eq!(body["tools"][0]["function"]["name"], "lookup");
    }
    #[test]
    fn parses_chat_tool_call_and_usage() {
        let body = r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c1","function":{"name":"lookup","arguments":"{\"q\":\"x\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":2,"completion_tokens":3}}"#;
        let c = parse_completion(LlmApi::OpenAiChat, body).unwrap();
        assert_eq!(c.tool_calls[0].arguments["q"], "x");
        assert_eq!(c.usage.unwrap().total_tokens, 5);
    }

    #[test]
    fn anthropic_preserves_remote_image_sources() {
        let req = CompletionRequest {
            messages: vec![ConversationMessage::Chat(ChatMessage {
                role: "user".into(),
                content: MessageContent::Parts(vec![ContentPart::ImageUrl {
                    image_url: crate::message::ImageUrl {
                        url: "https://cdn.example/image.png".into(),
                    },
                }]),
            })],
            tools: vec![],
        };
        let body = build_request(&cfg(LlmApi::AnthropicMessages), req).unwrap();
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
        assert_eq!(body["messages"][0]["content"][0]["source"]["type"], "url");
    }
}
