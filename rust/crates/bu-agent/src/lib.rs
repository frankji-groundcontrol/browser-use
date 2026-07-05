//! Autonomous browser-use agent loop.
//!
//! Perceive (DOM + optional screenshot) -> decide (LLM, multi-action + reasoning)
//! -> act (browser actor). Mirrors the Python MCP `retry_with_browser_use_agent`
//! tool: same report wording, `use_vision`, and provider-agnostic LLM backend.

mod action;
mod report;

use action::{parse_output, AgentAction, AgentOutput};
use bu_actor::ActorHandle;
use bu_dom::extract_clean_markdown;
use bu_llm::{message, message_with_image, LlmProvider};
use serde_json::json;

pub use report::AgentRunReport;

use report::push_unique_url;

const AGENT_SYSTEM_PROMPT: &str = r#"You drive a browser to complete the user's task.
Reply with exactly ONE JSON object and no prose, in this shape:
{"evaluation_previous_goal":"...","memory":"...","next_goal":"...","actions":[ ... ]}
- evaluation_previous_goal: did the previous step achieve its goal? (brief)
- memory: durable facts to carry across steps (what you have found/done so far).
- next_goal: what you intend to accomplish with these actions.
- actions: an ordered list of one or more actions to run this step. You may batch
  independent actions (e.g. several "type"s), but the batch stops after any
  navigation or click, so put those last.
Each action is one of:
{"action":"navigate","url":"https://example.com"}
{"action":"click","index":0}
{"action":"type","index":0,"text":"text"}
{"action":"scroll","direction":"down"}
{"action":"scroll","direction":"up"}
{"action":"extract","query":"question"}
{"action":"done","success":true,"result":"final answer"}
Indices refer to interactive_elements in the current state. When the task is
complete (or impossible), return a single "done" action with the result."#;

/// Runs an autonomous agent task against an existing browser actor.
///
/// When `use_vision` is set, each step attaches the page screenshot to the model
/// prompt (multimodal), matching the Python agent's default behaviour.
pub async fn run_task(
    task: impl Into<String>,
    max_steps: usize,
    actor: ActorHandle,
    llm: &LlmProvider,
    use_vision: bool,
) -> AgentRunReport {
    let task = task.into();
    let mut report = AgentRunReport::default();
    // `agent_memory` is the model's own running memory (replaced each step).
    // `read_state` persists extraction results + recent action errors across
    // steps so a datum found on step 3 is still visible on step 7.
    let mut agent_memory = String::new();
    let mut read_state: Vec<String> = Vec::new();
    let mut consecutive_failures = 0usize;
    let mut done = false;

    for step in 0..max_steps {
        report.steps += 1;
        let is_last_step = step + 1 == max_steps;

        let snapshot = match actor.get_state(use_vision).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                report.errors.push(format!("get_state failed: {error}"));
                break;
            }
        };
        push_unique_url(&mut report.urls_visited, snapshot.page.url.clone());
        let pre_url = snapshot.page.url.clone();

        let screenshot = snapshot.screenshot.clone();
        let state = json!({
            "url": snapshot.page.url,
            "title": snapshot.page.title,
            "interactive_elements": snapshot.elements.iter().map(|element| {
                json!({
                    "index": element.index,
                    "tag": element.tag,
                    "text": element.text
                })
            }).collect::<Vec<_>>()
        });
        let final_hint = if is_last_step {
            "\n\nThis is the FINAL step: return a single \"done\" action with your best answer."
        } else {
            ""
        };
        let user_text = format!(
            "<task>\n{task}\n</task>\n\n<memory>\n{memory}\n</memory>\n\n<read_state>\n{read_state}\n</read_state>\n\n<current_state>\n{state}\n</current_state>{final_hint}",
            memory = agent_memory,
            read_state = read_state.join("\n"),
            state = serde_json::to_string_pretty(&state).unwrap_or_else(|_| state.to_string()),
        );
        let user_message = match (use_vision, screenshot.as_deref()) {
            (true, Some(png)) => message_with_image("user", user_text, png),
            _ => message("user", user_text),
        };

        let response = match llm
            .chat(vec![message("system", AGENT_SYSTEM_PROMPT), user_message])
            .await
        {
            Ok(response) => response,
            Err(error) => {
                report.errors.push(format!("llm chat failed: {error}"));
                break;
            }
        };

        let output = match parse_output(&response) {
            Ok(output) => output,
            Err(error) => {
                report.errors.push(format!("invalid agent action: {error}"));
                push_read_state(
                    &mut read_state,
                    format!("could not parse your last reply: {error}"),
                );
                consecutive_failures += 1;
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    report.errors.push(format!(
                        "aborting after {consecutive_failures} consecutive failed steps"
                    ));
                    break;
                }
                continue;
            }
        };

        tracing::debug!(
            evaluation = %output.evaluation_previous_goal,
            next_goal = %output.next_goal,
            actions = output.actions.len(),
            "agent step"
        );

        let AgentOutput {
            memory, actions, ..
        } = output;
        if !memory.trim().is_empty() {
            agent_memory = memory;
        }

        // A reasoning-only turn (no actions) is a valid no-op step.
        if actions.is_empty() {
            consecutive_failures = 0;
            continue;
        }

        let action_count = actions.len();
        let mut step_error = false;
        let mut batch_done = false;
        for (index, action) in actions.into_iter().enumerate() {
            match execute_action(action, &actor, llm).await {
                Step::Continue {
                    rerender,
                    observation,
                } => {
                    if let Some(observation) = observation {
                        push_read_state(&mut read_state, observation);
                    }
                    if rerender {
                        break;
                    }
                    // Guard remaining batched actions against a page change a
                    // type/scroll may have triggered (stale indices otherwise).
                    if index + 1 < action_count {
                        if let Ok(state) = actor.page_state().await {
                            if state.url != pre_url {
                                break;
                            }
                        }
                    }
                }
                Step::Done { success, result } => {
                    report.success = success;
                    report.final_result = result;
                    batch_done = true;
                    done = true;
                    break;
                }
                Step::Error(error) => {
                    push_read_state(&mut read_state, format!("action failed: {error}"));
                    report.errors.push(error);
                    step_error = true;
                    break;
                }
            }
        }

        if batch_done {
            break;
        }
        if step_error {
            consecutive_failures += 1;
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                report.errors.push(format!(
                    "aborting after {consecutive_failures} consecutive failed steps"
                ));
                break;
            }
        } else {
            consecutive_failures = 0;
        }
    }

    // If the agent never emitted a `done`, synthesize a best-effort final answer
    // from what it gathered instead of returning an empty result.
    if !done {
        if let Ok(answer) = best_effort_final(&task, &agent_memory, &read_state, llm).await {
            if !answer.trim().is_empty() {
                report.final_result = answer;
            }
        }
    }

    report
}

/// Max consecutive failed steps before the run aborts (prevents burning the whole
/// step budget re-issuing the same failing action).
const MAX_CONSECUTIVE_FAILURES: usize = 3;
/// Cap on the persistent extraction/error buffer.
const READ_STATE_CAP: usize = 12;

fn push_read_state(buffer: &mut Vec<String>, item: String) {
    if item.trim().is_empty() {
        return;
    }
    buffer.push(item);
    let overflow = buffer.len().saturating_sub(READ_STATE_CAP);
    if overflow > 0 {
        buffer.drain(0..overflow);
    }
}

/// Asks the model for its best final answer when the run ended without a `done`.
async fn best_effort_final(
    task: &str,
    memory: &str,
    read_state: &[String],
    llm: &LlmProvider,
) -> anyhow::Result<String> {
    llm.chat(vec![
        message(
            "system",
            "You are finishing a browser task that ran out of steps. Give the best final answer you can from what was gathered. Reply with plain text, no JSON.",
        ),
        message(
            "user",
            format!(
                "<task>\n{task}\n</task>\n\n<memory>\n{memory}\n</memory>\n\n<gathered>\n{gathered}\n</gathered>\n\nProvide the best final answer you can.",
                gathered = read_state.join("\n"),
            ),
        ),
    ])
    .await
}

enum Step {
    /// Action succeeded. `rerender` forces re-observation before the next action
    /// (navigation/click may invalidate element indices); `observation` carries
    /// extraction text back into the agent's memory.
    Continue {
        rerender: bool,
        observation: Option<String>,
    },
    Done {
        success: bool,
        result: String,
    },
    Error(String),
}

async fn execute_action(action: AgentAction, actor: &ActorHandle, llm: &LlmProvider) -> Step {
    match action {
        AgentAction::Navigate { url } => match actor.navigate(url, false).await {
            Ok(()) => Step::Continue {
                rerender: true,
                observation: None,
            },
            Err(error) => Step::Error(format!("navigate failed: {error}")),
        },
        AgentAction::Click { index } => match actor.click(index, false).await {
            Ok(_) => Step::Continue {
                rerender: true,
                observation: None,
            },
            Err(error) => Step::Error(format!("click failed: {error}")),
        },
        AgentAction::Type { index, text } => match actor.type_text(index, text).await {
            Ok(()) => Step::Continue {
                rerender: false,
                observation: None,
            },
            Err(error) => Step::Error(format!("type failed: {error}")),
        },
        AgentAction::Scroll { direction } => {
            match actor.scroll(direction.as_str().to_owned()).await {
                Ok(()) => Step::Continue {
                    rerender: false,
                    observation: None,
                },
                Err(error) => Step::Error(format!("scroll failed: {error}")),
            }
        }
        AgentAction::Extract { query } => match extract_with_llm(actor, llm, &query).await {
            Ok(result) => Step::Continue {
                rerender: false,
                observation: Some(format!("extract({query}): {result}")),
            },
            Err(error) => Step::Error(format!("extract failed: {error}")),
        },
        AgentAction::Done { success, result } => Step::Done { success, result },
    }
}

async fn extract_with_llm(
    actor: &ActorHandle,
    llm: &LlmProvider,
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

#[cfg(all(test, feature = "live-chrome"))]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use bu_actor::ActorHandle;
    use bu_llm::{LlmProvider, OpenAiChatClient, OpenAiChatConfig};
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
        let provider = LlmProvider::OpenAi(llm);
        let actor = ActorHandle::spawn();
        let page = "data:text/html,<title>Agent Test</title><button onclick='document.body.dataset.clicked=\"yes\"'>Flip</button>";
        actor.navigate(page.to_owned(), false).await?;

        let report =
            crate::run_task("Click the Flip button", 4, actor.clone(), &provider, false).await;

        assert_eq!(report.steps, 2);
        assert!(report.success);
        assert_eq!(report.final_result, "clicked");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(
            report.to_python_report(),
            format!(
                "Task completed in 2 steps\nSuccess: True\n\nFinal result:\nclicked\n\nURLs visited: {page}"
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

    #[tokio::test]
    async fn run_task_attaches_screenshot_when_vision_enabled() -> anyhow::Result<()> {
        let llm_server = ScriptedLlmServer::spawn(vec![
            json!({"action": "done", "success": true, "result": "seen"}).to_string(),
        ]);
        let provider = LlmProvider::OpenAi(OpenAiChatClient::new(OpenAiChatConfig {
            api_key: "test-key".to_owned(),
            base_url: llm_server.base_url(),
            model: "mock-model".to_owned(),
            temperature: None,
        })?);
        let actor = ActorHandle::spawn();
        actor
            .navigate(
                "data:text/html,<title>Vision</title><main>hi</main>".to_owned(),
                false,
            )
            .await?;

        let report = crate::run_task("Describe the page", 2, actor.clone(), &provider, true).await;
        assert!(report.success, "{:?}", report.errors);

        let requests = llm_server.join();
        assert_eq!(requests.len(), 1);
        // With vision, the user message content is a multimodal parts array with an image_url.
        let parts = &requests[0]["messages"][1]["content"];
        assert!(
            parts.is_array(),
            "vision content should be an array: {parts:#?}"
        );
        assert_eq!(parts[0]["type"], "text");
        assert_eq!(parts[1]["type"], "image_url");
        assert!(parts[1]["image_url"]["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("data:image/png;base64,")));

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
