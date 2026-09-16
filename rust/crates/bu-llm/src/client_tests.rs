use crate::client::status_hint;

#[test]
fn browser_use_host_gets_actionable_hints() {
    let base = "https://llm.api.browser-use.com/v1";
    assert!(status_hint(base, 401)
        .unwrap()
        .contains("BROWSER_USE_LLM_API_KEY"));
    assert!(status_hint(base, 402).unwrap().contains("credits"));
    assert!(
        status_hint(base, 500).is_none(),
        "5xx is not operator-fixable"
    );
}

#[test]
fn other_hosts_keep_the_plain_error() {
    assert!(status_hint("https://api.openai.com/v1", 401).is_none());
    assert!(status_hint("https://api.anthropic.com/v1", 402).is_none());
}

mod http_tests {
    use crate::message::{message, message_with_image};
    use crate::{LlmApi, LlmClient, LlmConfig, ANTHROPIC_VERSION};
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        sync::mpsc,
        thread,
    };

    struct Reply {
        status: &'static str,
        content_type: &'static str,
        body: &'static str,
    }

    fn json_ok(body: &'static str) -> Reply {
        Reply {
            status: "200 OK",
            content_type: "application/json",
            body,
        }
    }

    /// Serves `replies` in order, returning the base URL and the request lines
    /// (request-line + headers + body) each attempt actually sent.
    fn serve(replies: Vec<Reply>) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let raw = drain_request(&mut stream);
                let _ = tx.send(String::from_utf8_lossy(&raw).into_owned());
                let _ = write!(
                    stream,
                    "HTTP/1.1 {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    reply.status,
                    reply.content_type,
                    reply.body.len(),
                    reply.body
                );
            }
        });
        (base_url, rx)
    }

    fn client(base_url: String, api: LlmApi) -> LlmClient {
        LlmClient::new(LlmConfig {
            api_key: "secret-key".to_owned(),
            base_url,
            model: "m".to_owned(),
            api,
            temperature: None,
            max_tokens: 77,
        })
        .unwrap()
    }

    fn request_line(raw: &str) -> String {
        raw.lines().next().unwrap_or_default().to_owned()
    }

    #[tokio::test]
    async fn exact_url_is_used_first_without_any_v1_guessing() {
        // Bare host + chat: the exact route is {base}/chat/completions.
        let (base_url, requests) = serve(vec![json_ok(
            r#"{"choices":[{"message":{"content":"ok"}}]}"#,
        )]);
        let out = client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        assert_eq!(out, "ok");
        let first = request_line(&requests.recv().unwrap());
        assert!(
            first.contains("POST /chat/completions "),
            "exact URL must be tried first, got: {first}"
        );
        assert!(
            requests.try_recv().is_err(),
            "a working exact URL must not trigger a fallback attempt"
        );
    }

    #[tokio::test]
    async fn a_404_at_the_exact_url_falls_back_to_the_v1_root() {
        let (base_url, requests) = serve(vec![
            Reply {
                status: "404 Not Found",
                content_type: "application/json",
                body: r#"{"error":"no route"}"#,
            },
            json_ok(r#"{"choices":[{"message":{"content":"recovered"}}]}"#),
        ]);
        let out = client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        assert_eq!(out, "recovered");
        assert!(request_line(&requests.recv().unwrap()).contains("POST /chat/completions "));
        assert!(
            request_line(&requests.recv().unwrap()).contains("POST /v1/chat/completions "),
            "fallback should append /v1"
        );
    }

    #[tokio::test]
    async fn an_html_landing_page_also_triggers_the_fallback() {
        let (base_url, requests) = serve(vec![
            Reply {
                status: "200 OK",
                content_type: "text/html; charset=utf-8",
                body: "<html><body>gateway</body></html>",
            },
            json_ok(r#"{"choices":[{"message":{"content":"recovered"}}]}"#),
        ]);
        let out = client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        assert_eq!(
            out, "recovered",
            "a 200 HTML page is a wrong route, not an answer"
        );
        drop(requests);
    }

    #[tokio::test]
    async fn neither_root_working_names_both_urls_tried() {
        let (base_url, _requests) = serve(vec![
            Reply {
                status: "404 Not Found",
                content_type: "application/json",
                body: "{}",
            },
            Reply {
                status: "404 Not Found",
                content_type: "application/json",
                body: "{}",
            },
        ]);
        let error = client(base_url, LlmApi::AnthropicMessages)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("/messages"),
            "should name the route: {error}"
        );
        assert!(
            error.contains("/v1/messages"),
            "should name both roots tried: {error}"
        );
        assert!(
            error.contains("BROWSER_USE_LLM_BASE_URL"),
            "should name the variable to fix: {error}"
        );
    }

    #[tokio::test]
    async fn retries_on_429_then_succeeds() {
        let (base_url, _requests) = serve(vec![
            Reply {
                status: "429 Too Many Requests",
                content_type: "application/json",
                body: r#"{"error":"slow down"}"#,
            },
            json_ok(r#"{"choices":[{"message":{"content":"recovered"}}]}"#),
        ]);
        let out = client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        assert_eq!(out, "recovered");
    }

    #[tokio::test]
    async fn anthropic_sends_its_own_auth_headers_and_body_shape() {
        let (base_url, requests) = serve(vec![json_ok(
            r#"{"content":[{"type":"text","text":"hi back"}]}"#,
        )]);
        let out = client(base_url, LlmApi::AnthropicMessages)
            .chat(vec![
                message("system", "be terse"),
                message_with_image("user", "look", b"\x89PNG\r\n\x1a\nrest"),
            ])
            .await
            .unwrap();
        assert_eq!(out, "hi back");

        let raw = requests.recv().unwrap();
        let lower = raw.to_ascii_lowercase();
        assert!(lower.contains("post /messages "), "wrong route: {raw}");
        assert!(
            lower.contains("x-api-key: secret-key"),
            "Anthropic authenticates with x-api-key, not bearer: {raw}"
        );
        assert!(
            lower.contains(&format!("anthropic-version: {ANTHROPIC_VERSION}")),
            "the dated version header is required: {raw}"
        );
        assert!(
            !lower.contains("authorization: bearer"),
            "must not also send OpenAI bearer auth: {raw}"
        );

        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default();
        let json: serde_json::Value = serde_json::from_str(body).expect("body is JSON");
        assert_eq!(json["system"], "be terse", "system must be hoisted");
        assert_eq!(json["max_tokens"], 77, "max_tokens is required");
        assert_eq!(json["messages"][0]["content"][1]["type"], "image");
        assert_eq!(
            json["messages"][0]["content"][1]["source"]["type"],
            "base64"
        );
    }

    #[tokio::test]
    async fn openai_sends_bearer_auth() {
        let (base_url, requests) = serve(vec![json_ok(
            r#"{"choices":[{"message":{"content":"ok"}}]}"#,
        )]);
        client(base_url, LlmApi::OpenAiChat)
            .chat(vec![message("user", "hi")])
            .await
            .unwrap();
        let lower = requests.recv().unwrap().to_ascii_lowercase();
        assert!(
            lower.contains("authorization: bearer secret-key"),
            "{lower}"
        );
        assert!(!lower.contains("x-api-key"), "must not send Anthropic auth");
    }

    fn drain_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let Ok(read) = stream.read(&mut chunk) else {
                break;
            };
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&buffer[..end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if buffer.len() >= end + 4 + content_length {
                    break;
                }
            }
        }
        buffer
    }
}
