//! Autonomous browser-use agent loop.

use bu_actor::ActorHandle;
use bu_dom::extract_clean_markdown;
use bu_llm::{message, OpenAiChatClient};
use serde::Deserialize;
use serde_json::json;

/// Summary returned by an autonomous agent run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunReport {
    /// Number of model-directed steps attempted.
    pub steps: usize,
    /// Whether the model marked the task as successful.
    pub success: bool,
    /// Final model-provided task result, if any.
    pub final_result: String,
    /// Per-step errors encountered during the run.
    pub errors: Vec<String>,
    /// URLs observed while the agent was running.
    pub urls_visited: Vec<String>,
}

impl AgentRunReport {
    /// Formats this report with the Python MCP retry tool wording.
    pub fn to_python_report(&self) -> String {
        format!(
            "Task completed in {steps} steps\nSuccess: {success}\nFinal result: {result}\nErrors encountered: {errors}\nURLs visited: {urls}",
            steps = self.steps,
            success = self.success,
            result = self.final_result,
            errors = serde_json::to_string(&self.errors).unwrap_or_else(|_| "[]".to_owned()),
            urls = self.urls_visited.join(",")
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum AgentAction {
    Navigate { url: String },
    Click { index: usize },
    Type { index: usize, text: String },
    Scroll { direction: ScrollDirection },
    Extract { query: String },
    Done { success: bool, result: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScrollDirection {
    Down,
    Up,
}

impl ScrollDirection {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
        }
    }
}

/// Runs an autonomous agent task against an existing browser actor.
pub async fn run_task(
    task: impl Into<String>,
    max_steps: usize,
    actor: ActorHandle,
    llm: OpenAiChatClient,
) -> AgentRunReport {
    let task = task.into();
    let mut report = AgentRunReport {
        steps: 0,
        success: false,
        final_result: String::new(),
        errors: Vec::new(),
        urls_visited: Vec::new(),
    };

    for _ in 0..max_steps {
        report.steps += 1;

        let snapshot = match actor.get_state(false).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                report.errors.push(format!("get_state failed: {error}"));
                break;
            }
        };
        push_unique_url(&mut report.urls_visited, snapshot.page.url.clone());

        let state = json!({
            "url": snapshot.page.url,
            "title": snapshot.page.title,
            "interactive_elements": snapshot.elements.into_iter().map(|element| {
                json!({
                    "index": element.index,
                    "tag": element.tag,
                    "text": element.text
                })
            }).collect::<Vec<_>>()
        });
        let response = match llm
            .chat(vec![
                message("system", AGENT_SYSTEM_PROMPT),
                message(
                    "user",
                    format!(
                        "<task>\n{task}\n</task>\n\n<current_state>\n{}\n</current_state>",
                        serde_json::to_string_pretty(&state).unwrap_or_else(|_| state.to_string())
                    ),
                ),
            ])
            .await
        {
            Ok(response) => response,
            Err(error) => {
                report.errors.push(format!("llm chat failed: {error}"));
                break;
            }
        };

        let action = match parse_action(&response) {
            Ok(action) => action,
            Err(error) => {
                report.errors.push(format!("invalid agent action: {error}"));
                break;
            }
        };

        match execute_action(action, &actor, &llm).await {
            ActionExecution::Continue => {}
            ActionExecution::Done { success, result } => {
                report.success = success;
                report.final_result = result;
                break;
            }
            ActionExecution::Error(error) => {
                report.errors.push(error);
            }
        }
    }

    report
}

const AGENT_SYSTEM_PROMPT: &str = r#"You drive a browser to complete the user's task.
You MUST reply with exactly one JSON object and no prose.
Choose one action:
{"action":"navigate","url":"https://example.com"}
{"action":"click","index":0}
{"action":"type","index":0,"text":"text"}
{"action":"scroll","direction":"down"}
{"action":"scroll","direction":"up"}
{"action":"extract","query":"question"}
{"action":"done","success":true,"result":"final answer"}"#;

enum ActionExecution {
    Continue,
    Done { success: bool, result: String },
    Error(String),
}

async fn execute_action(
    action: AgentAction,
    actor: &ActorHandle,
    llm: &OpenAiChatClient,
) -> ActionExecution {
    let result = match action {
        AgentAction::Navigate { url } => actor.navigate(url, false).await,
        AgentAction::Click { index } => actor.click(index, false).await.map(|_| ()),
        AgentAction::Type { index, text } => actor.type_text(index, text).await,
        AgentAction::Scroll { direction } => actor.scroll(direction.as_str().to_owned()).await,
        AgentAction::Extract { query } => {
            return match extract_with_llm(actor, llm, &query).await {
                Ok(_) => ActionExecution::Continue,
                Err(error) => ActionExecution::Error(format!("extract failed: {error}")),
            };
        }
        AgentAction::Done { success, result } => {
            return ActionExecution::Done { success, result };
        }
    };

    match result {
        Ok(()) => ActionExecution::Continue,
        Err(error) => ActionExecution::Error(format!("action failed: {error}")),
    }
}

async fn extract_with_llm(
    actor: &ActorHandle,
    llm: &OpenAiChatClient,
    query: &str,
) -> anyhow::Result<String> {
    let html = actor.get_html(None).await?;
    let (markdown, _) = extract_clean_markdown(&html, false);
    llm.chat(vec![
        message(
            "system",
            "You extract information from clean webpage markdown. Answer the query directly and concisely using only the webpage content.",
        ),
        message(
            "user",
            format!("<query>\n{query}\n</query>\n\n<webpage_content>\n{markdown}\n</webpage_content>"),
        ),
    ])
    .await
}

fn parse_action(response: &str) -> anyhow::Result<AgentAction> {
    let stripped = strip_code_fence(response.trim());
    Ok(serde_json::from_str(stripped)?)
}

fn strip_code_fence(text: &str) -> &str {
    let Some(after_opening) = text.strip_prefix("```") else {
        return text;
    };
    let after_language = after_opening
        .strip_prefix("json")
        .or_else(|| after_opening.strip_prefix("JSON"))
        .unwrap_or(after_opening)
        .trim_start_matches(['\r', '\n']);
    after_language
        .strip_suffix("```")
        .map(str::trim)
        .unwrap_or(text)
}

fn push_unique_url(urls: &mut Vec<String>, url: String) {
    if url.is_empty() || urls.iter().any(|seen| seen == &url) {
        return;
    }
    urls.push(url);
}

#[cfg(all(test, feature = "live-chrome"))]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use bu_actor::ActorHandle;
    use bu_llm::{OpenAiChatClient, OpenAiChatConfig};
    use serde_json::{json, Value};

    #[tokio::test]
    async fn run_task_executes_scripted_llm_actions_and_reports_python_format() -> anyhow::Result<()>
    {
        let llm_server = ScriptedLlmServer::spawn(vec![
            json!({"action": "click", "index": 0}).to_string(),
            json!({"action": "done", "success": true, "result": "clicked"}).to_string(),
        ]);
        let llm = OpenAiChatClient::new(OpenAiChatConfig {
            api_key: "test-key".to_owned(),
            base_url: llm_server.base_url(),
            model: "mock-model".to_owned(),
            temperature: None,
        })?;
        let actor = ActorHandle::spawn();
        let page = "data:text/html,<title>Agent Test</title><button onclick='document.body.dataset.clicked=\"yes\"'>Flip</button>";
        actor.navigate(page.to_owned(), false).await?;

        let report = crate::run_task("Click the Flip button", 4, actor.clone(), llm).await;

        assert_eq!(report.steps, 2);
        assert!(report.success);
        assert_eq!(report.final_result, "clicked");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(
            report.to_python_report(),
            format!(
                "Task completed in 2 steps\nSuccess: true\nFinal result: clicked\nErrors encountered: []\nURLs visited: {page}"
            )
        );
        assert_eq!(
            actor.evaluate("document.body.dataset.clicked").await?,
            json!("yes")
        );

        let requests = llm_server.join();
        assert_eq!(requests.len(), 2);
        assert!(requests
            .iter()
            .all(|request| request["model"] == "mock-model"));
        assert!(
            requests
                .iter()
                .all(|request| request["messages"][1]["content"]
                    .as_str()
                    .is_some_and(|content| content.contains("Click the Flip button")
                        && content.contains("interactive_elements"))),
            "{requests:#?}"
        );

        Ok(())
    }

    struct ScriptedLlmServer {
        base_url: String,
        handle: thread::JoinHandle<Vec<Value>>,
    }

    impl ScriptedLlmServer {
        fn spawn(responses: Vec<String>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock LLM server");
            let base_url = format!("http://{}", listener.local_addr().unwrap());
            let handle = thread::spawn(move || {
                let mut requests = Vec::new();
                for response in responses {
                    let (mut stream, _) = listener.accept().expect("accept LLM request");
                    let request = read_http_request(&mut stream);
                    let body = request
                        .split("\r\n\r\n")
                        .nth(1)
                        .expect("request should include body");
                    requests.push(serde_json::from_str(body).expect("request body is JSON"));
                    let response_body = json!({
                        "choices": [
                            {"message": {"content": response}}
                        ]
                    })
                    .to_string();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    )
                    .expect("write LLM response");
                }
                requests
            });

            Self { base_url, handle }
        }

        fn base_url(&self) -> String {
            self.base_url.clone()
        }

        fn join(self) -> Vec<Value> {
            self.handle.join().expect("mock LLM server thread")
        }
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        let mut content_length = None;
        loop {
            let read = stream.read(&mut chunk).expect("read LLM request");
            assert_ne!(read, 0, "connection closed before request body");
            buffer.extend_from_slice(&chunk[..read]);
            if content_length.is_none() {
                if let Some(header_end) = find_header_end(&buffer) {
                    let headers = String::from_utf8_lossy(&buffer[..header_end]);
                    content_length = headers.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("content-length"))
                    });
                }
            }
            if let (Some(header_end), Some(content_length)) =
                (find_header_end(&buffer), content_length)
            {
                if buffer.len() >= header_end + 4 + content_length {
                    return String::from_utf8(buffer).expect("request is utf8");
                }
            }
        }
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }
}
