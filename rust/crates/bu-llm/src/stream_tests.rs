use super::*;
use crate::stream::{SseDecoder, StreamState};
use crate::LlmApi;
use serde_json::json;

#[test]
fn finish_reason_does_not_discard_later_usage_or_accept_truncation() {
    let mut state = StreamState::new(LlmApi::OpenAiChat);
    state
        .event(
            r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            &mut |_| {},
        )
        .unwrap();
    assert!(!state.is_done(), "usage can follow finish_reason");
    state
        .event(
            r#"{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#,
            &mut |_| {},
        )
        .unwrap();
    state.event("[DONE]", &mut |_| {}).unwrap();
    assert_eq!(state.finish().unwrap().usage.unwrap().total_tokens, 7);
    let mut state = StreamState::new(LlmApi::OpenAiChat);
    assert!(state
        .event(r#"{"choices":[{"finish_reason":"length"}]}"#, &mut |_| {})
        .is_err());
    let mut state = StreamState::new(LlmApi::AnthropicMessages);
    state
        .event(
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
            &mut |_| {},
        )
        .unwrap();
    assert!(!state.is_done());
    assert!(state.finish().is_err());
}

#[test]
fn sparse_content_indices_do_not_create_phantom_tools() {
    for (api, events) in [
        (
            LlmApi::AnthropicMessages,
            vec![
                json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"c","name":"lookup","input":{}}}),
                json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
                json!({"type":"message_stop"}),
            ],
        ),
        (
            LlmApi::OpenAiResponses,
            vec![
                json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"c","name":"lookup"}}),
                json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":"{}"}),
                json!({"type":"response.completed","response":{"status":"completed"}}),
            ],
        ),
    ] {
        let mut state = StreamState::new(api);
        for event in events {
            state.event(&event.to_string(), &mut |_| {}).unwrap();
        }
        assert_eq!(state.finish().unwrap().tool_calls.len(), 1);
    }
}

#[test]
fn multiline_sse_event_has_a_total_size_limit() {
    let mut decoder = SseDecoder::default();
    let line = format!("data: {}\n", "x".repeat(1024 * 1024));
    for _ in 0..7 {
        decoder.push(line.as_bytes()).unwrap();
    }
    assert!(decoder.push(line.as_bytes()).is_err());
}

#[test]
fn responses_completed_does_not_emit_arguments_twice() {
    let mut state = StreamState::new(LlmApi::OpenAiResponses);
    let mut args = String::new();
    for event in [
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"c","name":"lookup"}}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"{}"}),
        json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"c","name":"lookup","arguments":"{}"}]}}),
    ] {
        state
            .event(&event.to_string(), &mut |e| {
                if let StreamEvent::ToolCallDelta { arguments, .. } = e {
                    args.push_str(&arguments);
                }
            })
            .unwrap();
    }
    assert_eq!(args, "{}");
    assert_eq!(state.finish().unwrap().tool_calls[0].arguments, json!({}));
}

#[test]
fn fragmented_sse_preserves_unicode_multiline_data_and_tool_arguments() {
    let events = [
        json!({"choices":[{"delta":{"content":"Hello 世"}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"q\":"}}]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"rust\"}"}}]},"finish_reason":"tool_calls"}]}),
        json!({"choices":[],"usage":{"prompt_tokens":4,"completion_tokens":7,"total_tokens":11}}),
    ];
    let wire = format!(
        ": ping\r\n\r\n{}data: [DONE]\r\n\r\n",
        events
            .iter()
            .map(|event| format!("data: {event}\r\n\r\n"))
            .collect::<String>()
    );
    let mut parser = SseDecoder::default();
    let mut state = StreamState::new(LlmApi::OpenAiChat);
    let mut deltas = Vec::new();
    for byte in wire.bytes() {
        for data in parser.push(&[byte]).unwrap() {
            state.event(&data, &mut |event| deltas.push(event)).unwrap();
        }
    }
    let completion = state.finish().unwrap();
    assert_eq!(completion.text, "Hello 世");
    assert_eq!(completion.tool_calls[0].arguments, json!({"q":"rust"}));
    assert_eq!(completion.usage.unwrap().total_tokens, 11);
    assert!(deltas
        .iter()
        .any(|event| matches!(event, StreamEvent::TextDelta(text) if text == "Hello 世")));
    let mut parser = SseDecoder::default();
    assert_eq!(
        parser.push(b"data: {\ndata: \"a\":1}\n\n").unwrap(),
        vec!["{\n\"a\":1}"]
    );
}

#[test]
fn anthropic_tool_stream_merges_usage_and_rejects_truncated_output() {
    let mut state = StreamState::new(LlmApi::AnthropicMessages);
    for event in [
        json!({"type":"message_start","message":{"usage":{"input_tokens":9,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"lookup","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"x\":1}"}}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ] {
        state.event(&event.to_string(), &mut |_| {}).unwrap();
    }
    let completion = state.finish().unwrap();
    assert_eq!(completion.tool_calls[0].arguments, json!({"x":1}));
    assert_eq!(completion.usage.unwrap().total_tokens, 14);
    let mut truncated = StreamState::new(LlmApi::AnthropicMessages);
    assert!(truncated
        .event(
            r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#,
            &mut |_| {}
        )
        .is_err());
}

#[test]
fn responses_stream_requires_terminal_event_and_returns_final_tools() {
    let mut state = StreamState::new(LlmApi::OpenAiResponses);
    state
        .event(
            r#"{"type":"response.output_text.delta","delta":"ok"}"#,
            &mut |_| {},
        )
        .unwrap();
    assert!(state.finish().is_err());
    let mut state = StreamState::new(LlmApi::OpenAiResponses);
    let event = json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","call_id":"c","name":"lookup","arguments":"{}"}],"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3}}});
    state.event(&event.to_string(), &mut |_| {}).unwrap();
    assert_eq!(state.finish().unwrap().tool_calls[0].id, "c");
    let mut state = StreamState::new(LlmApi::OpenAiChat);
    assert!(state
        .event(r#"{"error":{"message":"bad request"}}"#, &mut |_| {})
        .is_err());
}

#[test]
fn streamed_tool_arguments_require_an_object() {
    let mut state = StreamState::new(LlmApi::OpenAiChat);
    state.event(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"lookup","arguments":"null"}}]}}]}"#, &mut |_| {}).unwrap();
    state.event("[DONE]", &mut |_| {}).unwrap();
    assert!(state.finish().is_err());
}
