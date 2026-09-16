use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use crate::completion::{Completion, TokenUsage, ToolCall};
use crate::config::LlmApi;

/// A provider-neutral piece of a streamed completion.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    Usage(TokenUsage),
}

#[derive(Default)]
pub(crate) struct SseDecoder {
    line: Vec<u8>,
    data: Vec<String>,
}

impl SseDecoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.line);
                let line = String::from_utf8(line).context("LLM stream contained invalid UTF-8")?;
                let line = line.strip_suffix('\r').unwrap_or(&line);
                if line.is_empty() {
                    if !self.data.is_empty() {
                        out.push(self.data.join("\n"));
                        self.data.clear();
                    }
                } else if let Some(value) = line.strip_prefix("data:") {
                    self.data
                        .push(value.strip_prefix(' ').unwrap_or(value).to_owned());
                }
            } else {
                self.line.push(byte);
                if self.line.len() > 8 * 1024 * 1024 {
                    return Err(anyhow!("LLM stream event exceeded 8 MiB"));
                }
            }
        }
        Ok(out)
    }
}

#[derive(Default)]
struct PartialTool {
    id: String,
    name: String,
    arguments: String,
}

pub(crate) struct StreamState {
    api: LlmApi,
    text: String,
    tools: Vec<PartialTool>,
    usage: Option<TokenUsage>,
    finish_reason: Option<String>,
    done: bool,
}

impl StreamState {
    pub(crate) fn new(api: LlmApi) -> Self {
        Self {
            api,
            text: String::new(),
            tools: Vec::new(),
            usage: None,
            finish_reason: None,
            done: false,
        }
    }
    pub(crate) fn is_done(&self) -> bool {
        self.done
    }

    pub(crate) fn event(&mut self, data: &str, emit: &mut impl FnMut(StreamEvent)) -> Result<()> {
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        let value: Value =
            serde_json::from_str(data).context("failed to parse LLM stream event")?;
        if value.get("error").is_some() {
            return Err(anyhow!("LLM stream returned an error: {}", value["error"]));
        }
        match self.api {
            LlmApi::OpenAiChat => self.chat_event(&value, emit),
            LlmApi::OpenAiResponses => self.responses_event(&value, emit),
            LlmApi::AnthropicMessages => self.anthropic_event(&value, emit),
            #[cfg(feature = "bedrock")]
            LlmApi::Bedrock => Err(anyhow!("Bedrock HTTP streaming is unsupported")),
        }
    }

    fn chat_event(&mut self, value: &Value, emit: &mut impl FnMut(StreamEvent)) -> Result<()> {
        if let Some(usage) = value.get("usage").and_then(parse_openai_usage) {
            self.usage = Some(usage.clone());
            emit(StreamEvent::Usage(usage));
        }
        let Some(choice) = value["choices"].as_array().and_then(|items| items.first()) else {
            return Ok(());
        };
        if let Some(reason) = choice["finish_reason"].as_str() {
            self.finish_reason = Some(reason.to_owned());
            self.done = true;
        }
        if let Some(text) = choice["delta"]["content"].as_str() {
            self.text.push_str(text);
            emit(StreamEvent::TextDelta(text.to_owned()));
        }
        if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
            for call in calls {
                let index = call["index"].as_u64().unwrap_or(self.tools.len() as u64) as usize;
                self.ensure_tool(index);
                let tool = &mut self.tools[index];
                if let Some(id) = call["id"].as_str() {
                    tool.id = id.to_owned();
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    tool.name = name.to_owned();
                }
                let args = call["function"]["arguments"].as_str().unwrap_or_default();
                tool.arguments.push_str(args);
                emit(StreamEvent::ToolCallDelta {
                    index,
                    id: (!tool.id.is_empty()).then(|| tool.id.clone()),
                    name: (!tool.name.is_empty()).then(|| tool.name.clone()),
                    arguments: args.to_owned(),
                });
            }
        }
        Ok(())
    }

    fn responses_event(&mut self, value: &Value, emit: &mut impl FnMut(StreamEvent)) -> Result<()> {
        match value["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" => {
                if let Some(text) = value["delta"].as_str() {
                    self.text.push_str(text);
                    emit(StreamEvent::TextDelta(text.to_owned()));
                }
            }
            "response.function_call_arguments.delta" => {
                let index = value["output_index"]
                    .as_u64()
                    .unwrap_or(self.tools.len() as u64) as usize;
                self.ensure_tool(index);
                let args = value["delta"].as_str().unwrap_or_default();
                self.tools[index].arguments.push_str(args);
                emit(StreamEvent::ToolCallDelta {
                    index,
                    id: None,
                    name: None,
                    arguments: args.to_owned(),
                });
            }
            "response.output_item.added" => {
                if value["item"]["type"] == "function_call" {
                    let index = value["output_index"]
                        .as_u64()
                        .unwrap_or(self.tools.len() as u64)
                        as usize;
                    self.ensure_tool(index);
                    self.tools[index].id = value["item"]["call_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    self.tools[index].name = value["item"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                }
            }
            "response.completed" => {
                self.done = true;
                if let Some(response) = value.get("response") {
                    self.finish_reason = response["status"].as_str().map(ToOwned::to_owned);
                    if let Some(usage) = response.get("usage").and_then(parse_responses_usage) {
                        self.usage = Some(usage.clone());
                        emit(StreamEvent::Usage(usage));
                    }
                    if let Some(output) = response["output"].as_array() {
                        for item in output.iter().filter(|item| item["type"] == "function_call") {
                            let id = item["call_id"].as_str().unwrap_or_default().to_owned();
                            let index = self
                                .tools
                                .iter()
                                .position(|tool| tool.id == id)
                                .unwrap_or_else(|| {
                                    self.tools.push(PartialTool::default());
                                    self.tools.len() - 1
                                });
                            self.tools[index].id = id;
                            self.tools[index].name =
                                item["name"].as_str().unwrap_or_default().to_owned();
                            self.tools[index].arguments =
                                item["arguments"].as_str().unwrap_or("{}").to_owned();
                            emit(StreamEvent::ToolCallDelta {
                                index,
                                id: Some(self.tools[index].id.clone()),
                                name: Some(self.tools[index].name.clone()),
                                arguments: self.tools[index].arguments.clone(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn anthropic_event(&mut self, value: &Value, emit: &mut impl FnMut(StreamEvent)) -> Result<()> {
        match value["type"].as_str().unwrap_or_default() {
            "message_start" => {
                if let Some(usage) = value["message"]["usage"]
                    .as_object()
                    .and_then(parse_anthropic_usage)
                {
                    self.usage = Some(usage.clone());
                    emit(StreamEvent::Usage(usage));
                }
            }
            "content_block_start" => {
                if value["content_block"]["type"] == "tool_use" {
                    let index = value["index"].as_u64().unwrap_or(self.tools.len() as u64) as usize;
                    self.ensure_tool(index);
                    self.tools[index].id = value["content_block"]["id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    self.tools[index].name = value["content_block"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                }
            }
            "content_block_delta" => {
                let index = value["index"].as_u64().unwrap_or(0) as usize;
                if value["delta"]["type"] == "text_delta" {
                    if let Some(text) = value["delta"]["text"].as_str() {
                        self.text.push_str(text);
                        emit(StreamEvent::TextDelta(text.to_owned()));
                    }
                } else if value["delta"]["type"] == "input_json_delta" {
                    self.ensure_tool(index);
                    let args = value["delta"]["partial_json"].as_str().unwrap_or_default();
                    self.tools[index].arguments.push_str(args);
                    emit(StreamEvent::ToolCallDelta {
                        index,
                        id: None,
                        name: None,
                        arguments: args.to_owned(),
                    });
                }
            }
            "message_delta" => {
                if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                    self.finish_reason = Some(reason.to_owned());
                    self.done = true;
                    if reason == "max_tokens" {
                        return Err(anyhow!("Anthropic stream truncated at max_tokens"));
                    }
                }
                if let Some(output_tokens) = value["usage"]["output_tokens"].as_u64() {
                    let previous = self.usage.clone().unwrap_or_default();
                    let usage = TokenUsage {
                        input_tokens: previous.input_tokens,
                        output_tokens,
                        total_tokens: previous.input_tokens + output_tokens,
                    };
                    self.usage = Some(usage.clone());
                    emit(StreamEvent::Usage(usage));
                }
            }
            "message_stop" => self.done = true,
            _ => {}
        }
        Ok(())
    }

    fn ensure_tool(&mut self, index: usize) {
        while self.tools.len() <= index {
            self.tools.push(PartialTool::default());
        }
    }

    pub(crate) fn finish(self) -> Result<Completion> {
        if !self.done {
            return Err(anyhow!("LLM stream ended before a terminal event"));
        }
        let tool_calls = self
            .tools
            .into_iter()
            .map(|tool| {
                Ok(ToolCall {
                    id: tool.id,
                    name: tool.name,
                    arguments: serde_json::from_str(&tool.arguments)
                        .context("tool call arguments were not valid JSON")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Completion {
            text: self.text,
            tool_calls,
            usage: self.usage,
            finish_reason: self.finish_reason,
        })
    }
}

fn parse_openai_usage(value: &Value) -> Option<TokenUsage> {
    Some(TokenUsage {
        input_tokens: value["prompt_tokens"].as_u64()?,
        output_tokens: value["completion_tokens"].as_u64()?,
        total_tokens: value["total_tokens"].as_u64().unwrap_or(0),
    })
}
fn parse_responses_usage(value: &Value) -> Option<TokenUsage> {
    Some(TokenUsage {
        input_tokens: value["input_tokens"].as_u64()?,
        output_tokens: value["output_tokens"].as_u64()?,
        total_tokens: value["total_tokens"].as_u64().unwrap_or(0),
    })
}
fn parse_anthropic_usage(value: &serde_json::Map<String, Value>) -> Option<TokenUsage> {
    Some(TokenUsage {
        input_tokens: value.get("input_tokens")?.as_u64()?,
        output_tokens: value
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: value.get("input_tokens")?.as_u64()?
            + value
                .get("output_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0),
    })
}
