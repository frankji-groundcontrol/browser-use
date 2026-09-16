//! Conversion at the AWS Converse boundary; no transport or credential handling.
use anyhow::{anyhow, Context, Result};
use aws_sdk_bedrockruntime::types::*;
use aws_smithy_types::{Document, Number};
use serde_json::Value;

use crate::{Completion, CompletionRequest, ConversationMessage, TokenUsage, ToolCall};

pub(super) fn request(
    req: CompletionRequest,
) -> Result<(
    Vec<SystemContentBlock>,
    Vec<Message>,
    Option<ToolConfiguration>,
)> {
    crate::completion::validate(&req)?;
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for message in req.messages {
        let (role, blocks) = match message {
            ConversationMessage::Chat(chat) if chat.role == "system" => {
                system.push(SystemContentBlock::Text(chat.content.as_text()));
                continue;
            }
            ConversationMessage::Chat(chat) => {
                let role = match chat.role.as_str() {
                    "user" => ConversationRole::User,
                    "assistant" => ConversationRole::Assistant,
                    _ => return Err(anyhow!("unsupported Bedrock conversation role")),
                };
                (role, super::content_blocks(&chat.content)?)
            }
            ConversationMessage::Assistant { text, tool_calls } => {
                let mut blocks = Vec::new();
                if !text.is_empty() {
                    blocks.push(ContentBlock::Text(text));
                }
                for call in tool_calls {
                    blocks.push(ContentBlock::ToolUse(
                        ToolUseBlock::builder()
                            .tool_use_id(call.id)
                            .name(call.name)
                            .input(to_document(call.arguments))
                            .build()?,
                    ));
                }
                (ConversationRole::Assistant, blocks)
            }
            ConversationMessage::Tool { call_id, content } => {
                if call_id.is_empty() {
                    return Err(anyhow!("tool result requires a call id"));
                }
                (
                    ConversationRole::User,
                    vec![ContentBlock::ToolResult(
                        ToolResultBlock::builder()
                            .tool_use_id(call_id)
                            .content(ToolResultContentBlock::Text(content))
                            .build()?,
                    )],
                )
            }
        };
        if blocks.is_empty() {
            return Err(anyhow!("Bedrock message has no content"));
        }
        messages.push(
            Message::builder()
                .role(role)
                .set_content(Some(blocks))
                .build()?,
        );
    }
    let tools = if req.tools.is_empty() {
        None
    } else {
        let mut builder = ToolConfiguration::builder();
        for tool in req.tools {
            builder = builder.tools(Tool::ToolSpec(
                ToolSpecification::builder()
                    .name(tool.name)
                    .description(tool.description)
                    .input_schema(ToolInputSchema::Json(to_document(tool.parameters)))
                    .build()?,
            ));
        }
        Some(builder.build()?)
    };
    Ok((system, messages, tools))
}

pub(super) fn completion(
    output: &ConverseOutput,
    stop: &StopReason,
    usage: Option<&aws_sdk_bedrockruntime::types::TokenUsage>,
) -> Result<Completion> {
    check_stop(stop)?;
    let message = output
        .as_message()
        .map_err(|_| anyhow!("Bedrock output was not a message"))?;
    let mut completion = Completion {
        finish_reason: Some(stop.as_str().into()),
        usage: usage.map(token_usage).transpose()?,
        ..Default::default()
    };
    for block in message.content() {
        match block {
            ContentBlock::Text(text) => completion.text.push_str(text),
            ContentBlock::ToolUse(tool) => {
                let arguments = from_document(tool.input())?;
                if tool.tool_use_id().is_empty() || tool.name().is_empty() || !arguments.is_object()
                {
                    return Err(anyhow!("invalid Bedrock tool call"));
                }
                completion.tool_calls.push(ToolCall {
                    id: tool.tool_use_id().into(),
                    name: tool.name().into(),
                    arguments,
                });
            }
            _ => {}
        }
    }
    Ok(completion)
}

pub(super) fn check_stop(stop: &StopReason) -> Result<()> {
    match stop.as_str() {
        "end_turn" | "tool_use" | "stop_sequence" => Ok(()),
        _ => Err(anyhow!(
            "Bedrock response did not complete: {}",
            stop.as_str()
        )),
    }
}

pub(super) fn token_usage(usage: &aws_sdk_bedrockruntime::types::TokenUsage) -> Result<TokenUsage> {
    Ok(TokenUsage {
        input_tokens: usage
            .input_tokens()
            .try_into()
            .context("negative input usage")?,
        output_tokens: usage
            .output_tokens()
            .try_into()
            .context("negative output usage")?,
        total_tokens: usage
            .total_tokens()
            .try_into()
            .context("negative total usage")?,
    })
}

fn to_document(value: Value) -> Document {
    match value {
        Value::Null => Document::Null,
        Value::Bool(v) => Document::Bool(v),
        Value::String(v) => Document::String(v),
        Value::Array(v) => Document::Array(v.into_iter().map(to_document).collect()),
        Value::Object(v) => {
            Document::Object(v.into_iter().map(|(k, v)| (k, to_document(v))).collect())
        }
        Value::Number(v) => Document::Number(if let Some(v) = v.as_u64() {
            Number::PosInt(v)
        } else if let Some(v) = v.as_i64() {
            Number::NegInt(v)
        } else {
            Number::Float(v.as_f64().expect("JSON number"))
        }),
    }
}
fn from_document(doc: &Document) -> Result<Value> {
    Ok(match doc {
        Document::Null => Value::Null,
        Document::Bool(v) => Value::Bool(*v),
        Document::String(v) => Value::String(v.clone()),
        Document::Array(v) => Value::Array(v.iter().map(from_document).collect::<Result<_>>()?),
        Document::Object(v) => Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), from_document(v)?)))
                .collect::<Result<_>>()?,
        ),
        Document::Number(Number::PosInt(v)) => (*v).into(),
        Document::Number(Number::NegInt(v)) => (*v).into(),
        Document::Number(Number::Float(v)) => serde_json::Number::from_f64(*v)
            .context("non-finite tool argument")?
            .into(),
    })
}
