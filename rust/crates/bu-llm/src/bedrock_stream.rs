use std::collections::BTreeMap;

use anyhow::{anyhow, Context, Result};
use aws_sdk_bedrockruntime::types::{ContentBlockDelta, ContentBlockStart, ConverseStreamOutput};

use crate::{Completion, StreamEvent, ToolCall};

#[derive(Default)]
pub(super) struct State {
    result: Completion,
    tools: BTreeMap<i32, (String, String, String)>,
}

impl State {
    pub(super) fn event(
        &mut self,
        event: ConverseStreamOutput,
        emit: &mut impl FnMut(StreamEvent),
    ) -> Result<()> {
        match event {
            ConverseStreamOutput::ContentBlockStart(event) => {
                if let Some(ContentBlockStart::ToolUse(tool)) = event.start() {
                    if self.tools.len() >= 1024
                        || event.content_block_index() < 0
                        || self.tools.contains_key(&event.content_block_index())
                    {
                        return Err(anyhow!("invalid Bedrock tool index or too many tool calls"));
                    }
                    self.tools.insert(
                        event.content_block_index(),
                        (tool.tool_use_id().into(), tool.name().into(), String::new()),
                    );
                    emit(StreamEvent::ToolCallDelta {
                        index: event.content_block_index() as usize,
                        id: Some(tool.tool_use_id().into()),
                        name: Some(tool.name().into()),
                        arguments: String::new(),
                    });
                }
            }
            ConverseStreamOutput::ContentBlockDelta(event) => match event.delta() {
                Some(ContentBlockDelta::Text(text)) => {
                    self.result.text.push_str(text);
                    emit(StreamEvent::TextDelta(text.clone()));
                }
                Some(ContentBlockDelta::ToolUse(delta)) => {
                    let tool = self
                        .tools
                        .get_mut(&event.content_block_index())
                        .context("Bedrock tool delta preceded its start")?;
                    if tool.2.len() + delta.input().len() > 8 * 1024 * 1024 {
                        return Err(anyhow!("Bedrock tool arguments exceeded 8 MiB"));
                    }
                    tool.2.push_str(delta.input());
                    emit(StreamEvent::ToolCallDelta {
                        index: event.content_block_index() as usize,
                        id: Some(tool.0.clone()),
                        name: Some(tool.1.clone()),
                        arguments: delta.input().into(),
                    });
                }
                _ => {}
            },
            ConverseStreamOutput::MessageStop(event) => {
                super::wire::check_stop(event.stop_reason())?;
                self.result.finish_reason = Some(event.stop_reason().as_str().into());
            }
            ConverseStreamOutput::Metadata(event) => {
                if let Some(usage) = event.usage() {
                    let usage = super::wire::token_usage(usage)?;
                    emit(StreamEvent::Usage(usage.clone()));
                    self.result.usage = Some(usage);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<Completion> {
        if self.result.finish_reason.is_none() {
            return Err(anyhow!("Bedrock stream ended before messageStop"));
        }
        for (id, name, arguments) in self.tools.into_values() {
            let arguments: serde_json::Value =
                serde_json::from_str(&arguments).context("invalid Bedrock tool arguments")?;
            if id.is_empty() || name.is_empty() || !arguments.is_object() {
                return Err(anyhow!("invalid Bedrock tool call"));
            }
            self.result.tool_calls.push(ToolCall {
                id,
                name,
                arguments,
            });
        }
        Ok(self.result)
    }
}
