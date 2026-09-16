use super::*;
use crate::{CompletionRequest, ConversationMessage, LlmProvider, ToolCall, ToolDefinition};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

fn tool_request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![
            ConversationMessage::Chat(crate::message("system", "Be precise")),
            ConversationMessage::Chat(crate::message("user", "Lookup")),
            ConversationMessage::Assistant {
                text: String::new(),
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "lookup".into(),
                    arguments: json!({"q":"x"}),
                }],
            },
            ConversationMessage::Tool {
                call_id: "c1".into(),
                content: "found".into(),
            },
        ],
        tools: vec![ToolDefinition {
            name: "lookup".into(),
            description: "Look up".into(),
            parameters: json!({"type":"object","properties":{"q":{"type":"string"}}}),
        }],
    }
}

struct Reply {
    status: u16,
    mime: &'static str,
    body: Vec<u8>,
    delay: Duration,
}
fn reply(body: Value) -> Reply {
    Reply {
        status: 200,
        mime: "application/json",
        body: body.to_string().into_bytes(),
        delay: Duration::ZERO,
    }
}
fn serve(
    replies: Vec<Reply>,
    timeout: Duration,
    attempts: u32,
) -> (LlmProvider, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        for response in replies {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(_) if std::time::Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => return,
                }
            };
            let _ = stream.set_nonblocking(false);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                if std::time::Instant::now() >= deadline {
                    return;
                }
                let mut buf = [0u8; 4096];
                match stream.read(&mut buf) {
                    Ok(0) => return,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        continue;
                    }
                    Err(_) => return,
                }
                if let Some(end) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let Some(len) = headers
                        .lines()
                        .find_map(|s| s.strip_prefix("content-length:"))
                        .and_then(|s| s.trim().parse::<usize>().ok())
                    else {
                        continue;
                    };
                    if request.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let _ = tx.send(String::from_utf8(request).unwrap());
            thread::sleep(response.delay);
            let header = format!("HTTP/1.1 {} Fixture\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", response.status, response.mime, response.body.len());
            let _ = stream.write_all(header.as_bytes());
            for fragment in response.body.chunks(13) {
                if stream.write_all(fragment).is_err() {
                    break;
                }
            }
        }
    });
    let config = aws_sdk_bedrockruntime::config::Builder::new()
        .behavior_version_latest()
        .region(aws_sdk_bedrockruntime::config::Region::new("us-east-1"))
        .credentials_provider(aws_sdk_bedrockruntime::config::Credentials::new(
            "fixture", "fixture", None, None, "fixture",
        ))
        .endpoint_url(endpoint)
        .retry_config(aws_smithy_types::retry::RetryConfig::standard().with_max_attempts(attempts))
        .timeout_config(
            aws_smithy_types::timeout::TimeoutConfig::builder()
                .operation_timeout(timeout)
                .build(),
        )
        .build();
    (
        LlmProvider::Bedrock(BedrockChatClient {
            client: Client::from_conf(config),
            model: "fixture".into(),
            inference: InferenceConfiguration::builder().max_tokens(77).build(),
        }),
        rx,
    )
}

#[tokio::test]
async fn typed_provider_uses_converse_tools_usage_and_sdk_retry() {
    let mut transient = reply(json!({"message":"try again"}));
    transient.status = 503;
    let output = reply(
        json!({"output":{"message":{"role":"assistant","content":[{"text":"ok"},{"toolUse":{"toolUseId":"c2","name":"lookup","input":{"q":"next","n":3,"negative":-2,"decimal":1.5,"flag":true,"list":[null]}}}]}},"stopReason":"tool_use","usage":{"inputTokens":2,"outputTokens":3,"totalTokens":5},"metrics":{"latencyMs":1}}),
    );
    let (provider, requests) = serve(vec![transient, output], Duration::from_secs(5), 2);
    let result = provider.complete(tool_request()).await.unwrap();
    assert_eq!(result.text, "ok");
    assert_eq!(result.tool_calls[0].id, "c2");
    assert_eq!(result.tool_calls[0].arguments["negative"], -2);
    assert_eq!(result.usage.unwrap().total_tokens, 5);
    for _ in 0..2 {
        let raw = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(raw.starts_with("POST /model/fixture/converse "));
        assert!(raw
            .to_lowercase()
            .contains("authorization: aws4-hmac-sha256"));
        let body: Value = serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["system"][0]["text"], "Be precise");
        assert_eq!(
            body["messages"][1]["content"][0]["toolUse"]["input"],
            json!({"q":"x"})
        );
        assert_eq!(
            body["messages"][2]["content"][0]["toolResult"]["toolUseId"],
            "c1"
        );
        assert_eq!(body["toolConfig"]["tools"][0]["toolSpec"]["name"], "lookup");
        assert_eq!(body["inferenceConfig"]["maxTokens"], 77);
    }
}

// AWS event-stream framing: big-endian lengths, typed string headers and CRC32.
fn crc(bytes: &[u8]) -> u32 {
    let mut value = !0u32;
    for byte in bytes {
        value ^= *byte as u32;
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(value & 1)));
        }
    }
    !value
}
fn frame(kind: &str, body: Value) -> Vec<u8> {
    let mut headers = Vec::new();
    for (name, value) in [
        (":message-type", "event"),
        (":event-type", kind),
        (":content-type", "application/json"),
    ] {
        headers.push(name.len() as u8);
        headers.extend_from_slice(name.as_bytes());
        headers.push(7);
        headers.extend_from_slice(&(value.len() as u16).to_be_bytes());
        headers.extend_from_slice(value.as_bytes());
    }
    let payload = body.to_string();
    let mut out = Vec::new();
    out.extend_from_slice(&((16 + headers.len() + payload.len()) as u32).to_be_bytes());
    out.extend_from_slice(&(headers.len() as u32).to_be_bytes());
    out.extend_from_slice(&crc(&out).to_be_bytes());
    out.extend(headers);
    out.extend_from_slice(payload.as_bytes());
    out.extend_from_slice(&crc(&out).to_be_bytes());
    out
}
fn stream_reply(terminal: bool) -> Reply {
    let mut body = Vec::new();
    for (kind, event) in [
        ("messageStart", json!({"role":"assistant"})),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":0,"delta":{"text":"ok"}}),
        ),
        (
            "contentBlockStart",
            json!({"contentBlockIndex":1,"start":{"toolUse":{"toolUseId":"c2","name":"lookup"}}}),
        ),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":1,"delta":{"toolUse":{"input":"{\"q\":"}}}),
        ),
        (
            "contentBlockDelta",
            json!({"contentBlockIndex":1,"delta":{"toolUse":{"input":"\"next\"}"}}}),
        ),
        ("contentBlockStop", json!({"contentBlockIndex":1})),
    ] {
        body.extend(frame(kind, event));
    }
    if terminal {
        body.extend(frame("messageStop", json!({"stopReason":"tool_use"})));
        body.extend(frame("metadata", json!({"usage":{"inputTokens":2,"outputTokens":3,"totalTokens":5},"metrics":{"latencyMs":1}})));
    }
    Reply {
        status: 200,
        mime: "application/vnd.amazon.eventstream",
        body,
        delay: Duration::ZERO,
    }
}

#[tokio::test]
async fn converse_stream_decodes_frames_and_preserves_usage_after_stop() {
    let (provider, requests) = serve(vec![stream_reply(true)], Duration::from_secs(5), 1);
    let mut deltas = String::new();
    let result = provider
        .stream(tool_request(), |e| {
            if let StreamEvent::ToolCallDelta { arguments, .. } = e {
                deltas.push_str(&arguments);
            }
        })
        .await
        .unwrap();
    assert_eq!(result.text, "ok");
    assert_eq!(result.tool_calls[0].arguments, json!({"q":"next"}));
    assert_eq!(result.usage.unwrap().total_tokens, 5);
    assert_eq!(deltas, "{\"q\":\"next\"}");
    assert!(requests
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .starts_with("POST /model/fixture/converse-stream "));
}

#[tokio::test]
async fn bedrock_rejects_truncation_bad_crc_and_timeout() {
    let (provider, _) = serve(vec![stream_reply(false)], Duration::from_secs(5), 1);
    let err = provider
        .stream(tool_request(), |_| {})
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("messageStop"), "{err}");
    let mut broken = stream_reply(true);
    *broken.body.last_mut().unwrap() ^= 1;
    let (provider, _) = serve(vec![broken], Duration::from_secs(5), 1);
    assert!(provider.stream(tool_request(), |_| {}).await.is_err());
    let mut slow = reply(json!({}));
    slow.delay = Duration::from_millis(300);
    let (provider, _) = serve(vec![slow], Duration::from_millis(50), 1);
    let start = std::time::Instant::now();
    assert!(provider.complete(tool_request()).await.is_err());
    assert!(start.elapsed() < Duration::from_millis(250));
}
