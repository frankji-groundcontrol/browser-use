//! Minimal MCP server for the Rust browser-use rewrite.

use std::{
    future::{self, Future},
    sync::Arc,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bu_actor::{ActorHandle, ClickOutcome};
use bu_dom::extract_clean_markdown;
use bu_llm::{message, OpenAiChatClient};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResult, ContentBlock, ErrorCode, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::{RequestContext, RoleServer},
    transport::io::stdio,
    ErrorData, ServerHandler, ServiceExt,
};
use serde_json::{json, Map, Value};

const SESSION_ID: &str = "default";

mod tools;
pub use tools::low_level_tools;

/// Minimal rmcp server implementation for the browser-use MCP surface.
#[derive(Debug, Clone)]
pub struct BrowserUseMcpServer {
    actor: ActorHandle,
    // Serializes autonomous agent runs so they can't drive the single shared
    // browser concurrently or interleave their scoped allowed_domains policy
    // save/restore (which would corrupt the base policy).
    agent_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Default for BrowserUseMcpServer {
    fn default() -> Self {
        Self {
            actor: ActorHandle::spawn(),
            agent_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl BrowserUseMcpServer {
    /// Creates a new browser-use MCP server.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a server whose actor reports Chromium launches to `launch_counter`.
    #[cfg(feature = "live-chrome")]
    pub fn with_browser_launch_counter(
        launch_counter: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        Self {
            actor: ActorHandle::spawn_with_launch_counter(launch_counter),
            agent_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// Returns the underlying actor handle for live browser integration tests.
    #[cfg(feature = "live-chrome")]
    pub fn actor(&self) -> ActorHandle {
        self.actor.clone()
    }

    async fn call_browser_tool(
        &self,
        request: CallToolRequestParams,
    ) -> Result<CallToolResult, ErrorData> {
        if !low_level_tools()
            .iter()
            .any(|tool| tool.name == request.name)
        {
            return Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("Unknown tool: {}", request.name),
                None,
            ));
        }

        match request.name.as_ref() {
            "browser_navigate" => self.navigate(request.arguments).await,
            "browser_get_state" => self.get_state(request.arguments).await,
            "browser_click" => self.click(request.arguments).await,
            "browser_type" => self.type_text(request.arguments).await,
            "browser_get_html" => self.get_html(request.arguments).await,
            "browser_extract_content" => self.extract_content(request.arguments).await,
            "browser_screenshot" => self.screenshot(request.arguments).await,
            "browser_scroll" => self.scroll(request.arguments).await,
            "browser_go_back" => self.go_back().await,
            "browser_list_tabs" => self.list_tabs().await,
            "browser_switch_tab" => self.switch_tab(request.arguments).await,
            "browser_close_tab" => self.close_tab(request.arguments).await,
            "retry_with_browser_use_agent" => self.retry_agent(request.arguments).await,
            "browser_list_sessions" => self.list_sessions().await,
            "browser_close_session" => self.close_session(request.arguments).await,
            "browser_close_all" => self.close_all().await,
            name => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "{name} is not implemented yet"
            ))])),
        }
    }

    async fn navigate(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let url = arguments
            .as_ref()
            .and_then(|args| args.get("url"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ErrorData::new(
                    ErrorCode::INVALID_PARAMS,
                    "browser_navigate requires a string url argument",
                    None,
                )
            })?;
        let new_tab = optional_bool(arguments.as_ref(), "new_tab").unwrap_or(false);

        if let Err(error) = self.actor.navigate(url.to_owned(), new_tab).await {
            return Ok(browser_tool_error("browser_navigate failed", error));
        }

        let message = if new_tab {
            format!("Opened new tab with URL: {url}")
        } else {
            format!("Navigated to: {url}")
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(message)]))
    }

    async fn get_state(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let include_screenshot =
            optional_bool(arguments.as_ref(), "include_screenshot").unwrap_or(false);
        // Recoverable browser failures return a tool error (isError), not a
        // JSON-RPC protocol error, so the model can read and react to them.
        let snapshot = match self.actor.get_state(include_screenshot).await {
            Ok(snapshot) => snapshot,
            Err(error) => return Ok(browser_tool_error("browser_get_state failed", error)),
        };
        let interactive_elements = snapshot
            .elements
            .into_iter()
            .map(|element| {
                json!({
                    "index": element.index,
                    "tag": element.tag,
                    "text": element.text,
                    "href": element.href
                })
            })
            .collect::<Vec<_>>();
        let tabs = snapshot
            .tabs
            .into_iter()
            .map(|tab| {
                json!({
                    "id": tab.id,
                    "tab_id": tab.id,
                    "target_id": tab.target_id,
                    "url": tab.url,
                    "title": tab.title,
                    "active": tab.active
                })
            })
            .collect::<Vec<_>>();

        let payload = json!({
            "url": snapshot.page.url,
            "title": snapshot.page.title,
            "interactive_elements": interactive_elements,
            "tabs": tabs
        });

        // Return the screenshot as an image content block (like Python), not
        // base64 nested in the JSON.
        let mut result = CallToolResult::structured(payload);
        if let Some(screenshot) = snapshot.screenshot {
            result.content.push(ContentBlock::image(
                BASE64_STANDARD.encode(screenshot),
                "image/png",
            ));
        }
        Ok(result)
    }

    async fn click(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let coordinate_x = optional_i64(arguments.as_ref(), "coordinate_x");
        let coordinate_y = optional_i64(arguments.as_ref(), "coordinate_y");
        if let (Some(x), Some(y)) = (coordinate_x, coordinate_y) {
            if let Err(error) = self.actor.click_coordinates(x as f64, y as f64).await {
                return Ok(browser_tool_error("browser_click failed", error));
            }

            return Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                "Clicked at coordinates ({x}, {y})"
            ))]));
        }
        let has_index = arguments
            .as_ref()
            .and_then(|args| args.get("index"))
            .is_some();
        // A single stray coordinate is an error only when no index is given;
        // otherwise fall through to index-based click (matching Python).
        if (coordinate_x.is_some() || coordinate_y.is_some()) && !has_index {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "browser_click requires both coordinate_x and coordinate_y when using coordinate mode",
                None,
            ));
        }

        let index = required_usize(arguments.as_ref(), "index", "browser_click")?;
        let new_tab = optional_bool(arguments.as_ref(), "new_tab").unwrap_or(false);
        let outcome = match self.actor.click(index, new_tab).await {
            Ok(outcome) => outcome,
            Err(error) => return Ok(browser_tool_error("browser_click failed", error)),
        };

        let message = match outcome {
            ClickOutcome::Clicked => format!("Clicked element {index}"),
            ClickOutcome::OpenedNewTab(url) => {
                format!(
                    "Clicked element {index} and opened in new tab {}...",
                    prefix_chars(&url, 20)
                )
            }
            ClickOutcome::NewTabUnsupported => {
                format!("Clicked element {index} (new tab not supported for non-link elements)")
            }
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(message)]))
    }

    async fn type_text(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let index = required_usize(arguments.as_ref(), "index", "browser_type")?;
        let text = required_str(arguments.as_ref(), "text", "browser_type")?.to_owned();
        if let Err(error) = self.actor.type_text(index, text.clone()).await {
            return Ok(browser_tool_error("browser_type failed", error));
        }

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Typed {} into element {index}",
            masked_typed_text(&text)
        ))]))
    }

    async fn get_html(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let selector = optional_str(arguments.as_ref(), "selector");
        let html = match self.actor.get_html(selector.map(str::to_owned)).await {
            Ok(html) => html,
            Err(error) => return Ok(browser_tool_error("browser_get_html failed", error)),
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(html)]))
    }

    async fn retry_agent(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        // Serialize agent runs: only one drives the shared browser at a time, and
        // the scoped-policy save/restore below is reentrancy-safe under overlap.
        let _agent_guard = self.agent_lock.lock().await;

        let task =
            required_str(arguments.as_ref(), "task", "retry_with_browser_use_agent")?.to_owned();
        let max_steps = optional_i64(arguments.as_ref(), "max_steps")
            .unwrap_or(100)
            .max(1) as usize;
        let model = optional_str(arguments.as_ref(), "model").map(str::to_owned);
        // Python's retry tool defaults use_vision to true.
        let use_vision = optional_bool(arguments.as_ref(), "use_vision").unwrap_or(true);
        // A non-empty allowed_domains confines the agent to those domains for this
        // run (matching Python's `if allowed_domains:` override); empty/absent = no
        // override. Restore the base policy afterward.
        let allowed_domains =
            optional_string_list(arguments.as_ref(), "allowed_domains").filter(|d| !d.is_empty());

        // Build the LLM BEFORE mutating the policy, so a config error can't
        // `?`-return past the restore and leak the scoped policy into the session.
        let provider = build_agent_llm(model).await?;

        let restore = match allowed_domains {
            Some(domains) => match self.actor.get_policy().await {
                Ok(base) => {
                    let scoped = bu_actor::BrowserUrlPolicy {
                        allowed_domains: domains,
                        ..base.clone()
                    };
                    match self.actor.set_policy(scoped).await {
                        Ok(_) => Some(base),
                        Err(error) => {
                            return Ok(browser_tool_error("browser policy update failed", error))
                        }
                    }
                }
                Err(error) => return Ok(browser_tool_error("browser policy read failed", error)),
            },
            None => None,
        };

        // No `?` between set_policy(scoped) and the restore below; run_task
        // returns a report rather than propagating errors, so restore always runs.
        let report =
            bu_agent::run_task(task, max_steps, self.actor.clone(), &provider, use_vision).await;

        if let Some(base) = restore {
            let _ = self.actor.set_policy(base).await;
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(
            report.to_python_report(),
        )]))
    }

    async fn extract_content(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = required_str(arguments.as_ref(), "query", "browser_extract_content")?;
        let extract_links = optional_bool(arguments.as_ref(), "extract_links").unwrap_or(false);

        let html = match self.actor.get_html(None).await {
            Ok(html) => html,
            Err(error) => return Ok(browser_tool_error("browser_extract_content failed", error)),
        };
        let page = match self.actor.page_state().await {
            Ok(page) => page,
            Err(error) => return Ok(browser_tool_error("browser_extract_content failed", error)),
        };
        let (markdown, _) = extract_clean_markdown(&html, extract_links);
        // Cap very large pages so a single extraction request stays bounded.
        let markdown = cap_markdown(&markdown, EXTRACT_MARKDOWN_CHAR_LIMIT);

        let system_prompt = "You extract information from clean webpage markdown. Answer the query directly and concisely using only the webpage content.";
        let user_prompt = format!(
            "<query>\n{query}\n</query>\n\n<webpage_content>\n{markdown}\n</webpage_content>"
        );
        let answer = OpenAiChatClient::from_env()
            .map_err(llm_error)?
            .chat(vec![
                message("system", system_prompt),
                message("user", user_prompt),
            ])
            .await
            .map_err(llm_error)?;

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "<url>{}</url>\n<query>{query}</query>\n<result>{}</result>",
            page.url,
            answer.trim()
        ))]))
    }

    async fn screenshot(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let full_page = optional_bool(arguments.as_ref(), "full_page").unwrap_or(false);
        let png = match self.actor.screenshot(full_page).await {
            Ok(png) => png,
            Err(error) => return Ok(browser_tool_error("browser_screenshot failed", error)),
        };
        let metadata = json!({ "size_bytes": png.len() }).to_string();

        Ok(CallToolResult::success(vec![
            ContentBlock::text(metadata),
            ContentBlock::image(BASE64_STANDARD.encode(png), "image/png"),
        ]))
    }

    async fn scroll(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let direction = optional_str(arguments.as_ref(), "direction").unwrap_or("down");
        if !matches!(direction, "up" | "down") {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "browser_scroll direction must be 'up' or 'down'",
                None,
            ));
        }

        if let Err(error) = self.actor.scroll(direction.to_owned()).await {
            return Ok(browser_tool_error("browser_scroll failed", error));
        }

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Scrolled {direction}"
        ))]))
    }

    async fn go_back(&self) -> Result<CallToolResult, ErrorData> {
        if let Err(error) = self.actor.go_back().await {
            return Ok(browser_tool_error("browser_go_back failed", error));
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(
            "Navigated back",
        )]))
    }

    async fn list_tabs(&self) -> Result<CallToolResult, ErrorData> {
        let tabs = match self.actor.list_tabs().await {
            Ok(tabs) => tabs,
            Err(error) => return Ok(browser_tool_error("failed to list browser tabs", error)),
        }
        .into_iter()
        .map(|tab| {
            json!({
                "id": tab.id,
                "tab_id": tab.id,
                "target_id": tab.target_id,
                "url": tab.url,
                "title": tab.title,
                "active": tab.active
            })
        })
        .collect::<Vec<_>>();
        let text = serde_json::to_string_pretty(&tabs).map_err(json_error("browser_list_tabs"))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    async fn switch_tab(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let tab_id = required_str(arguments.as_ref(), "tab_id", "browser_switch_tab")?;
        let state = match self.actor.switch_tab(tab_id.to_owned()).await {
            Ok(state) => state,
            Err(error) => return Ok(browser_tool_error("browser_switch_tab failed", error)),
        };

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Switched to tab {tab_id}: {url}",
            url = state.url
        ))]))
    }

    async fn close_tab(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let tab_id = required_str(arguments.as_ref(), "tab_id", "browser_close_tab")?;
        let current_url = match self.actor.close_tab(tab_id.to_owned()).await {
            Ok(current_url) => current_url,
            Err(error) => return Ok(browser_tool_error("browser_close_tab failed", error)),
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Closed tab # {tab_id}, now on {current_url}"
        ))]))
    }

    async fn list_sessions(&self) -> Result<CallToolResult, ErrorData> {
        let session_url = match self.actor.list_sessions().await {
            Ok(session_url) => session_url,
            Err(error) => return Ok(browser_tool_error("browser_list_sessions failed", error)),
        };
        let Some(url) = session_url else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "No active browser sessions",
            )]));
        };
        let sessions = json!([
            {
                "session_id": SESSION_ID,
                "active": true,
                "current_url": url
            }
        ]);
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&sessions).map_err(json_error("browser_list_sessions"))?,
        )]))
    }

    async fn close_session(
        &self,
        arguments: Option<Map<String, Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let session_id = required_str(arguments.as_ref(), "session_id", "browser_close_session")?;
        if session_id != SESSION_ID {
            return Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                "Session {session_id} not found"
            ))]));
        }

        if let Err(error) = self.actor.close_session(session_id.to_owned()).await {
            return Ok(browser_tool_error("browser_close_session failed", error));
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Successfully closed session {SESSION_ID}"
        ))]))
    }

    async fn close_all(&self) -> Result<CallToolResult, ErrorData> {
        let had_session = match self.actor.close_all().await {
            Ok(had_session) => had_session,
            Err(error) => return Ok(browser_tool_error("browser_close_all failed", error)),
        };
        let message = if had_session {
            "Closed 1 sessions"
        } else {
            "No active sessions to close"
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(message)]))
    }
}

impl ServerHandler for BrowserUseMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("browser-use", env!("CARGO_PKG_VERSION")),
        )
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        future::ready(Ok(ListToolsResult::with_all_items(low_level_tools())))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        low_level_tools()
            .into_iter()
            .find(|tool| tool.name.as_ref() == name)
    }

    // rmcp's ServerHandler declares this as `-> impl Future`; keep the trait's
    // signature shape rather than an `async fn` desugaring.
    #[allow(clippy::manual_async_fn)]
    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        async move { self.call_browser_tool(request).await }
    }
}

fn browser_tool_error(message: &'static str, error: anyhow::Error) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!("{message}: {error}"))])
}

fn json_error(message: &'static str) -> impl FnOnce(serde_json::Error) -> ErrorData {
    move |error| {
        ErrorData::new(
            ErrorCode::INTERNAL_ERROR,
            message,
            Some(json!({ "error": error.to_string() })),
        )
    }
}

fn llm_error(error: anyhow::Error) -> ErrorData {
    ErrorData::new(
        ErrorCode::INTERNAL_ERROR,
        "browser_extract_content failed",
        Some(json!({ "error": error.to_string() })),
    )
}

/// Builds the agent LLM backend, mirroring Python's provider selection: AWS
/// Bedrock when `MODEL_PROVIDER=bedrock` (requires the `bedrock` build feature),
/// otherwise an OpenAI-compatible client.
async fn build_agent_llm(model: Option<String>) -> Result<bu_llm::LlmProvider, ErrorData> {
    #[cfg(feature = "bedrock")]
    {
        let is_bedrock = std::env::var("MODEL_PROVIDER")
            .map(|value| value.eq_ignore_ascii_case("bedrock"))
            .unwrap_or(false);
        if is_bedrock {
            // Python ignores the tool `model` arg for MODEL_PROVIDER=bedrock
            // (clients often pass an OpenAI model name); use MODEL/env only.
            let client = bu_llm::BedrockChatClient::from_env_with_model_override(None)
                .await
                .map_err(llm_error)?;
            return Ok(bu_llm::LlmProvider::Bedrock(client));
        }
    }

    let config =
        bu_llm::OpenAiChatConfig::from_env_with_model_override(model).map_err(llm_error)?;
    let client = OpenAiChatClient::new(config).map_err(llm_error)?;
    Ok(bu_llm::LlmProvider::OpenAi(client))
}

fn optional_str<'a>(arguments: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a str> {
    arguments
        .and_then(|args| args.get(key))
        .and_then(Value::as_str)
}

/// Character budget for the markdown fed to a single extraction request.
const EXTRACT_MARKDOWN_CHAR_LIMIT: usize = 30_000;

/// Truncates markdown to `limit` characters on a char boundary, noting the cut.
fn cap_markdown(markdown: &str, limit: usize) -> String {
    if markdown.chars().count() <= limit {
        return markdown.to_owned();
    }
    let truncated: String = markdown.chars().take(limit).collect();
    format!("{truncated}\n\n[content truncated at {limit} characters]")
}

fn optional_string_list(arguments: Option<&Map<String, Value>>, key: &str) -> Option<Vec<String>> {
    arguments
        .and_then(|args| args.get(key))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
}

fn optional_bool(arguments: Option<&Map<String, Value>>, key: &str) -> Option<bool> {
    arguments
        .and_then(|args| args.get(key))
        .and_then(Value::as_bool)
}

fn optional_i64(arguments: Option<&Map<String, Value>>, key: &str) -> Option<i64> {
    arguments
        .and_then(|args| args.get(key))
        .and_then(Value::as_i64)
}

fn masked_typed_text(text: &str) -> String {
    if let Some(sensitive_key_name) = sensitive_key_name(text) {
        format!("<{sensitive_key_name}>")
    } else {
        format!("'{text}'")
    }
}

fn sensitive_key_name(text: &str) -> Option<&'static str> {
    if text.len() < 6 {
        return None;
    }

    if text
        .split_once('@')
        .is_some_and(|(_, domain)| domain.contains('.'))
    {
        return Some("email");
    }

    let credential_like = text.len() >= 16
        && text.chars().any(|char| char.is_ascii_digit())
        && text.chars().any(|char| char.is_ascii_alphabetic())
        && text.chars().any(|char| matches!(char, '.' | '-' | '_'));
    credential_like.then_some("credential")
}

fn prefix_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn required_str<'a>(
    arguments: Option<&'a Map<String, Value>>,
    key: &str,
    tool_name: &str,
) -> Result<&'a str, ErrorData> {
    optional_str(arguments, key).ok_or_else(|| {
        ErrorData::new(
            ErrorCode::INVALID_PARAMS,
            format!("{tool_name} requires a string {key} argument"),
            None,
        )
    })
}

fn required_usize(
    arguments: Option<&Map<String, Value>>,
    key: &str,
    tool_name: &str,
) -> Result<usize, ErrorData> {
    arguments
        .and_then(|args| args.get(key))
        .and_then(|value| match value {
            Value::Number(number) => number
                .as_u64()
                .and_then(|value| usize::try_from(value).ok()),
            _ => None,
        })
        .ok_or_else(|| {
            ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                format!("{tool_name} requires an unsigned integer {key} argument"),
                None,
            )
        })
}

/// Runs the browser-use MCP server over stdio.
pub async fn run_stdio_server() -> anyhow::Result<()> {
    let service = BrowserUseMcpServer::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Returns the 15 low-level browser-use tools exposed by the MVP server.
#[cfg(test)]
mod tests;
