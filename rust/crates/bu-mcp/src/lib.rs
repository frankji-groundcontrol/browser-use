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

/// Minimal rmcp server implementation for the browser-use MCP surface.
#[derive(Debug, Clone)]
pub struct BrowserUseMcpServer {
    actor: ActorHandle,
}

impl Default for BrowserUseMcpServer {
    fn default() -> Self {
        Self {
            actor: ActorHandle::spawn(),
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
pub fn low_level_tools() -> Vec<Tool> {
    vec![
        tool(
            "browser_navigate",
            "Navigate to a URL in the browser",
            json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "The URL to navigate to"},
                    "new_tab": {"type": "boolean", "description": "Whether to open in a new tab", "default": false}
                },
                "required": ["url"]
            }),
        ),
        tool(
            "browser_click",
            "Click an element by index or at specific viewport coordinates. Use index for elements from browser_get_state, or coordinate_x/coordinate_y for pixel-precise clicking.",
            json!({
                "type": "object",
                "properties": {
                    "index": {
                        "type": "integer",
                        "description": "The index of the element to click (from browser_get_state). Provide this OR coordinate_x+coordinate_y."
                    },
                    "coordinate_x": {
                        "type": "integer",
                        "description": "X coordinate in pixels from the left edge of the viewport. Must be used together with coordinate_y. Provide this OR index."
                    },
                    "coordinate_y": {
                        "type": "integer",
                        "description": "Y coordinate in pixels from the top edge of the viewport. Must be used together with coordinate_x. Provide this OR index."
                    },
                    "new_tab": {
                        "type": "boolean",
                        "description": "Whether to open any resulting navigation in a new tab",
                        "default": false
                    }
                }
            }),
        ),
        tool(
            "browser_type",
            "Type text into an input field. Clears existing text by default; pass text=\"\" to clear only.",
            json!({
                "type": "object",
                "properties": {
                    "index": {
                        "type": "integer",
                        "description": "The index of the input element (from browser_get_state)"
                    },
                    "text": {
                        "type": "string",
                        "description": "The text to type. Pass an empty string (\"\") to clear the field without typing."
                    }
                },
                "required": ["index", "text"]
            }),
        ),
        tool(
            "browser_get_state",
            "Get the current state of the page including all interactive elements",
            json!({
                "type": "object",
                "properties": {
                    "include_screenshot": {
                        "type": "boolean",
                        "description": "Whether to include a screenshot of the current page",
                        "default": false
                    }
                }
            }),
        ),
        tool(
            "browser_extract_content",
            "Extract structured content from the current page based on a query",
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "What information to extract from the page"},
                    "extract_links": {
                        "type": "boolean",
                        "description": "Whether to include links in the extraction",
                        "default": false
                    }
                },
                "required": ["query"]
            }),
        ),
        tool(
            "browser_get_html",
            "Get the raw HTML of the current page or a specific element by CSS selector",
            json!({
                "type": "object",
                "properties": {
                    "selector": {
                        "type": "string",
                        "description": "Optional CSS selector to get HTML of a specific element. If omitted, returns full page HTML."
                    }
                }
            }),
        ),
        tool(
            "browser_screenshot",
            "Take a screenshot of the current page. Returns viewport metadata as text and the screenshot as an image.",
            json!({
                "type": "object",
                "properties": {
                    "full_page": {
                        "type": "boolean",
                        "description": "Whether to capture the full scrollable page or just the visible viewport",
                        "default": false
                    }
                }
            }),
        ),
        tool(
            "browser_scroll",
            "Scroll the page",
            json!({
                "type": "object",
                "properties": {
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down"],
                        "description": "Direction to scroll",
                        "default": "down"
                    }
                }
            }),
        ),
        tool(
            "browser_go_back",
            "Go back to the previous page",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "browser_list_tabs",
            "List all open tabs",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "browser_switch_tab",
            "Switch to a different tab",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": {"type": "string", "description": "4 Character Tab ID of the tab to switch to"}
                },
                "required": ["tab_id"]
            }),
        ),
        tool(
            "browser_close_tab",
            "Close a tab",
            json!({
                "type": "object",
                "properties": {
                    "tab_id": {"type": "string", "description": "4 Character Tab ID of the tab to close"}
                },
                "required": ["tab_id"]
            }),
        ),
        tool(
            "retry_with_browser_use_agent",
            "Retry a task using the browser-use agent. Only use this as a last resort if you fail to interact with a page multiple times.",
            json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The high-level goal and detailed step-by-step description of the task the AI browser agent needs to attempt, along with any relevant data needed to complete the task and info about previous attempts."
                    },
                    "max_steps": {
                        "type": "integer",
                        "description": "Maximum number of steps an agent can take.",
                        "default": 100
                    },
                    "model": {
                        "type": "string",
                        "description": "LLM model to use (e.g., gpt-4o, claude-3-opus-20240229). Defaults to the configured model."
                    },
                    "allowed_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "List of domains the agent is allowed to visit (security feature). Omit to use the server-configured profile defaults. An empty list is treated the same as omitting the argument and will NOT disable server-configured restrictions."
                    },
                    "use_vision": {
                        "type": "boolean",
                        "description": "Whether to use vision capabilities (screenshots) for the agent",
                        "default": true
                    }
                },
                "required": ["task"]
            }),
        ),
        tool(
            "browser_list_sessions",
            "List all active browser sessions with their details and last activity time",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "browser_close_session",
            "Close a specific browser session by its ID",
            json!({
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "The browser session ID to close (get from browser_list_sessions)"
                    }
                },
                "required": ["session_id"]
            }),
        ),
        tool(
            "browser_close_all",
            "Close all active browser sessions and clean up resources",
            json!({"type": "object", "properties": {}}),
        ),
    ]
}

fn tool(name: &'static str, description: &'static str, input_schema: Value) -> Tool {
    Tool::new(name, description, schema_object(input_schema))
}

fn schema_object(value: Value) -> Arc<Map<String, Value>> {
    let Value::Object(object) = value else {
        unreachable!("tool schemas are object literals")
    };
    Arc::new(object)
}

#[cfg(test)]
mod tests {
    use super::low_level_tools;
    #[cfg(feature = "live-chrome")]
    use super::BrowserUseMcpServer;
    #[cfg(feature = "live-chrome")]
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    #[cfg(feature = "live-chrome")]
    use rmcp::model::CallToolRequestParams;
    use serde_json::json;
    #[cfg(feature = "live-chrome")]
    use serde_json::{Map, Value};
    #[cfg(feature = "live-chrome")]
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    #[cfg(feature = "live-chrome")]
    use tokio::task::JoinSet;
    #[cfg(feature = "live-chrome")]
    use tokio::time::{timeout, Duration};

    #[test]
    fn tools_list_returns_16_low_level_tools() {
        let tools = low_level_tools();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();

        assert_eq!(tools.len(), 16);
        assert_eq!(
            names,
            [
                "browser_navigate",
                "browser_click",
                "browser_type",
                "browser_get_state",
                "browser_extract_content",
                "browser_get_html",
                "browser_screenshot",
                "browser_scroll",
                "browser_go_back",
                "browser_list_tabs",
                "browser_switch_tab",
                "browser_close_tab",
                "retry_with_browser_use_agent",
                "browser_list_sessions",
                "browser_close_session",
                "browser_close_all",
            ]
        );
        let extract = tools
            .iter()
            .find(|tool| tool.name.as_ref() == "browser_extract_content")
            .expect("extract tool should be listed");
        assert_eq!(
            serde_json::to_value(extract.input_schema.as_ref()).unwrap(),
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "What information to extract from the page"},
                    "extract_links": {
                        "type": "boolean",
                        "description": "Whether to include links in the extraction",
                        "default": false
                    }
                },
                "required": ["query"]
            })
        );
        let retry = tools
            .iter()
            .find(|tool| tool.name.as_ref() == "retry_with_browser_use_agent")
            .expect("retry_with_browser_use_agent tool should be listed");
        assert_eq!(
            retry.description.as_deref(),
            Some(
            "Retry a task using the browser-use agent. Only use this as a last resort if you fail to interact with a page multiple times."
            )
        );
        assert_eq!(
            serde_json::to_value(retry.input_schema.as_ref()).unwrap(),
            json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The high-level goal and detailed step-by-step description of the task the AI browser agent needs to attempt, along with any relevant data needed to complete the task and info about previous attempts."
                    },
                    "max_steps": {
                        "type": "integer",
                        "description": "Maximum number of steps an agent can take.",
                        "default": 100
                    },
                    "model": {
                        "type": "string",
                        "description": "LLM model to use (e.g., gpt-4o, claude-3-opus-20240229). Defaults to the configured model."
                    },
                    "allowed_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "List of domains the agent is allowed to visit (security feature). Omit to use the server-configured profile defaults. An empty list is treated the same as omitting the argument and will NOT disable server-configured restrictions."
                    },
                    "use_vision": {
                        "type": "boolean",
                        "description": "Whether to use vision capabilities (screenshots) for the agent",
                        "default": true
                    }
                },
                "required": ["task"]
            })
        );
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn navigate_then_get_state_uses_live_browser() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": "data:text/html,<title>MCP Live</title><h1>OK</h1>"}),
            ))
            .await?;

        let result = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?;
        let state = result
            .structured_content
            .expect("browser_get_state should return structured JSON");

        assert_eq!(state["title"], "MCP Live");
        assert!(state["url"].as_str().unwrap().starts_with("data:text/html"));
        assert_eq!(state["tabs"].as_array().unwrap().len(), 1);

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn low_level_tools_use_live_browser_without_dom_serializer() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": "data:text/html,<title>Low Tools</title><main id='app'>OK</main><div style='height:2000px'></div>"}),
            ))
            .await?;

        let html = server
            .call_browser_tool(call("browser_get_html", json!({"selector": "#app"})))
            .await?;
        assert_eq!(text_content(&html), "<main id=\"app\">OK</main>");

        let screenshot = server
            .call_browser_tool(call("browser_screenshot", json!({"full_page": false})))
            .await?;
        assert_eq!(screenshot.content.len(), 2);
        assert!(text_content(&screenshot).contains("\"size_bytes\""));
        assert!(matches!(
            screenshot.content.get(1),
            Some(rmcp::model::ContentBlock::Image(image))
                if image.mime_type == "image/png" && !image.data.is_empty()
        ));

        assert_eq!(
            text_content(
                &server
                    .call_browser_tool(call("browser_scroll", json!({"direction": "down"})))
                    .await?
            ),
            "Scrolled down"
        );

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": "data:text/html,<title>Second</title>"}),
            ))
            .await?;
        assert_eq!(
            text_content(
                &server
                    .call_browser_tool(call("browser_go_back", json!({})))
                    .await?
            ),
            "Navigated back"
        );

        let tabs = server
            .call_browser_tool(call("browser_list_tabs", json!({})))
            .await?;
        assert!(text_content(&tabs).contains("Low Tools"));

        let sessions = server
            .call_browser_tool(call("browser_list_sessions", json!({})))
            .await?;
        assert!(text_content(&sessions).contains("default"));

        let closed = server
            .call_browser_tool(call(
                "browser_close_session",
                json!({"session_id": "default"}),
            ))
            .await?;
        assert_eq!(text_content(&closed), "Successfully closed session default");

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn selector_map_powers_state_click_and_type() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({
                    "url": "data:text/html,<title>Selectors</title><button id=b onclick='this.dataset.clicked=\"yes\"'>Go</button><input id=i>"
                }),
            ))
            .await?;

        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let elements = state["interactive_elements"]
            .as_array()
            .expect("state should include interactive elements");
        let button_index = indexed_element(elements, "button", "Go");
        let input_index = indexed_element(elements, "input", "");

        server
            .call_browser_tool(call("browser_click", json!({"index": button_index})))
            .await?;
        server
            .call_browser_tool(call(
                "browser_type",
                json!({"index": input_index, "text": "typed"}),
            ))
            .await?;

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn selector_map_detects_modern_interactive_elements_and_filters_hidden(
    ) -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();
        let html = r##"
            <title>Modern Selectors</title>
            <a id="semantic" href="/semantic-destination">Semantic Link</a>
            <div id="inline" onclick="document.body.dataset.inline = 'yes'">Inline Listener</div>
            <div id="listener">Registered Listener</div>
            <button id="transparent" style="opacity:0" onclick="document.body.dataset.hidden = 'opacity'">Transparent</button>
            <button id="gone" style="display:none" onclick="document.body.dataset.hidden = 'display'">Gone</button>
            <script>
                document.getElementById('listener').addEventListener('click', () => {
                    document.body.dataset.registered = 'yes';
                });
            </script>
        "##;

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": format!("data:text/html,{html}")}),
            ))
            .await?;

        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let elements = state["interactive_elements"]
            .as_array()
            .expect("elements should be an array");

        assert!(
            has_element(elements, "a", "Semantic Link"),
            "semantic links must remain in the selector map: {elements:?}"
        );
        assert!(
            has_element(elements, "div", "Inline Listener"),
            "div onclick elements must be detected: {elements:?}"
        );
        assert!(
            has_element(elements, "div", "Registered Listener"),
            "div addEventListener('click') elements must be detected: {elements:?}"
        );
        assert!(
            !has_element(elements, "button", "Transparent"),
            "opacity:0 elements must be hidden from selector map: {elements:?}"
        );
        assert!(
            !has_element(elements, "button", "Gone"),
            "display:none elements must be hidden from selector map: {elements:?}"
        );

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn selector_map_drops_button_fully_covered_by_opaque_modal() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();
        let html = r#"
            <title>Modal Cover</title>
            <body style="margin:0">
              <button style="position:absolute;left:80px;top:80px;width:160px;height:50px;z-index:1" onclick="document.body.dataset.clicked='target'">Covered Target</button>
              <div id="modal" style="position:fixed;inset:0;background:rgb(0, 0, 0);opacity:1;z-index:9999"></div>
            </body>
        "#;

        server
            .call_browser_tool(call("browser_navigate", json!({"url": data_url(html)})))
            .await?;

        let covered_state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let covered_elements = covered_state["interactive_elements"]
            .as_array()
            .expect("elements should be an array");
        assert!(
            !has_element(covered_elements, "button", "Covered Target"),
            "fully covered button must be removed from selector map: {covered_elements:?}"
        );

        server
            .actor()
            .evaluate("document.getElementById('modal').remove()")
            .await?;

        let uncovered_state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let uncovered_elements = uncovered_state["interactive_elements"]
            .as_array()
            .expect("elements should be an array");
        assert!(
            has_element(uncovered_elements, "button", "Covered Target"),
            "button must return to selector map after modal removal: {uncovered_elements:?}"
        );

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn selector_map_keeps_one_index_for_button_wrapping_icon_and_text() -> anyhow::Result<()>
    {
        let server = BrowserUseMcpServer::new();
        let html = r#"
            <title>Wrapped Button</title>
            <button style="display:inline-flex;align-items:center;gap:4px;width:120px;height:44px">
              <svg width="16" height="16" aria-hidden="true"></svg>
              <span>Buy</span>
            </button>
        "#;

        server
            .call_browser_tool(call("browser_navigate", json!({"url": data_url(html)})))
            .await?;

        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let elements = state["interactive_elements"]
            .as_array()
            .expect("elements should be an array");
        let buy_elements = elements
            .iter()
            .filter(|element| element["text"] == "Buy")
            .collect::<Vec<_>>();

        assert_eq!(
            buy_elements.len(),
            1,
            "button with wrapped icon/text must produce one indexed element: {elements:?}"
        );
        assert_eq!(buy_elements[0]["tag"], "button");

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn selector_map_excludes_contained_tabbable_child_of_button() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();
        let html = r#"
            <title>Contained Child</title>
            <button style="display:inline-block;width:180px;height:56px">
              <span tabindex="0" style="display:block;width:100%;height:100%">Nested Action</span>
            </button>
        "#;

        server
            .call_browser_tool(call("browser_navigate", json!({"url": data_url(html)})))
            .await?;

        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let elements = state["interactive_elements"]
            .as_array()
            .expect("elements should be an array");
        let nested_elements = elements
            .iter()
            .filter(|element| element["text"] == "Nested Action")
            .collect::<Vec<_>>();

        assert_eq!(
            nested_elements.len(),
            1,
            "contained tabbable child must be excluded in favor of parent button: {elements:?}"
        );
        assert_eq!(nested_elements[0]["tag"], "button");

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn selector_map_keeps_form_control_inside_link() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();
        let html = r#"
            <title>Nested Form Control</title>
            <a href="/checkout" style="display:inline-block;padding:12px">
              Checkout
              <input value="Nested Control" style="display:block;width:160px;height:32px">
            </a>
        "#;

        server
            .call_browser_tool(call("browser_navigate", json!({"url": data_url(html)})))
            .await?;

        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let elements = state["interactive_elements"]
            .as_array()
            .expect("elements should be an array");

        assert!(
            has_element(elements, "input", "Nested Control"),
            "form control inside link must remain indexed: {elements:?}"
        );

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn indexed_click_after_scroll_uses_viewport_normalized_coordinates() -> anyhow::Result<()>
    {
        let server = BrowserUseMcpServer::new();
        // NB: inline styles only — a data: URL treats '#' as a fragment delimiter,
        // so `<style>#id{}</style>` would truncate the page (id selectors need '#').
        let html = r#"<title>Scrolled Click</title><body style="margin:0"><div style="height:900px"></div><button style="display:block;width:180px;height:80px;margin-left:32px" onclick="document.body.dataset.clicked = 'target'">Scrolled Target</button></body>"#;

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": format!("data:text/html,{html}")}),
            ))
            .await?;
        server
            .call_browser_tool(call("browser_scroll", json!({"direction": "down"})))
            .await?;

        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let target_index = indexed_element(
            state["interactive_elements"]
                .as_array()
                .expect("elements should be an array"),
            "button",
            "Scrolled Target",
        );

        server
            .call_browser_tool(call("browser_click", json!({"index": target_index})))
            .await?;

        let clicked = server
            .actor()
            .evaluate("document.body.dataset.clicked")
            .await?;
        assert_eq!(clicked, json!("target"));

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[cfg(feature = "live-chrome")]
    async fn concurrent_get_state_requests_share_one_browser_launch() -> anyhow::Result<()> {
        timeout(Duration::from_secs(20), async {
            let launch_count = Arc::new(AtomicUsize::new(0));
            let server = Arc::new(BrowserUseMcpServer::with_browser_launch_counter(
                launch_count.clone(),
            ));

            server
                .call_browser_tool(call(
                    "browser_navigate",
                    json!({"url": "data:text/html,<title>Concurrent</title><button>ready</button>"}),
                ))
                .await?;

            let mut tasks = JoinSet::new();
            for _ in 0..8 {
                let server = server.clone();
                tasks.spawn(async move {
                    server
                        .call_browser_tool(call(
                            "browser_get_state",
                            json!({"include_screenshot": false}),
                        ))
                        .await
                });
            }

            while let Some(result) = tasks.join_next().await {
                let state = result??;
                assert!(
                    state.structured_content.is_some(),
                    "get_state should return structured JSON"
                );
            }

            assert_eq!(launch_count.load(Ordering::SeqCst), 1);
            Ok::<(), anyhow::Error>(())
        })
        .await?
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[cfg(feature = "live-chrome")]
    async fn click_uses_cached_backend_node_id_after_dom_reorder() -> anyhow::Result<()> {
        timeout(Duration::from_secs(20), async {
            let server = BrowserUseMcpServer::new();
            let html = r#"
                <title>Stable Click</title>
                <main id="buttons">
                  <button id="first" onclick="document.body.dataset.clicked='first'">First</button>
                  <button id="second" onclick="document.body.dataset.clicked='second'">Second</button>
                </main>
                <script>
                  window.reorderButtons = () => {
                    const buttons = document.getElementById('buttons');
                    buttons.insertBefore(document.getElementById('second'), document.getElementById('first'));
                  };
                </script>
            "#;

            server
                .call_browser_tool(call(
                    "browser_navigate",
                    json!({"url": format!("data:text/html,{html}")}),
                ))
                .await?;

            let state = server
                .call_browser_tool(call("browser_get_state", json!({})))
                .await?
                .structured_content
                .expect("browser_get_state should return structured JSON");
            let first_index = indexed_element(
                state["interactive_elements"].as_array().expect("elements should be an array"),
                "button",
                "First",
            );

            server
                .call_browser_tool(call(
                    "browser_get_html",
                    json!({"selector": "body"}),
                ))
                .await?;
            server
                .actor()
                .evaluate("window.reorderButtons()")
                .await?;

            server
                .call_browser_tool(call("browser_click", json!({"index": first_index})))
                .await?;

            let html = text_content(
                &server
                    .call_browser_tool(call("browser_get_html", json!({"selector": "body"})))
                    .await?,
            )
            .to_owned();
            assert!(html.contains("data-clicked=\"first\""), "{html}");
            assert!(!html.contains("data-clicked=\"second\""), "{html}");

            Ok::<(), anyhow::Error>(())
        })
        .await?
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn coordinate_only_click_flips_positioned_button_state() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": "data:text/html,<title>Coordinate Click</title><button id=b style='position:absolute;left:40px;top:50px;width:80px;height:40px' onclick='this.dataset.clicked=\"yes\"'>Hit</button>"}),
            ))
            .await?;

        let result = server
            .call_browser_tool(call(
                "browser_click",
                json!({"coordinate_x": 80, "coordinate_y": 70}),
            ))
            .await?;
        assert_eq!(text_content(&result), "Clicked at coordinates (80, 70)");

        let clicked = server
            .actor()
            .evaluate("document.getElementById('b').dataset.clicked")
            .await?;
        assert_eq!(clicked, json!("yes"));

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn new_tab_click_on_link_opens_resolved_url() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();
        let base = "https://example.com/base/index.html";

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": format!("data:text/html,<base href='{base}'><title>Links</title><a href='next/page.html'>Next</a>")}),
            ))
            .await?;

        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        let link_index = indexed_element(
            state["interactive_elements"]
                .as_array()
                .expect("elements should be an array"),
            "a",
            "Next",
        );

        let result = server
            .call_browser_tool(call(
                "browser_click",
                json!({"index": link_index, "new_tab": true}),
            ))
            .await?;
        assert_eq!(
            text_content(&result),
            "Clicked element 0 and opened in new tab https://example.com/..."
        );

        let tabs = server
            .call_browser_tool(call("browser_list_tabs", json!({})))
            .await?;
        let tabs: Value = serde_json::from_str(text_content(&tabs))?;
        let tabs = tabs.as_array().expect("tabs should be an array");
        assert_eq!(tabs.len(), 2);
        assert!(tabs.iter().any(|tab| tab["url"]
            .as_str()
            .is_some_and(|url| url == "https://example.com/base/next/page.html")));

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn typing_replaces_prefilled_value_and_masks_email_result() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": "data:text/html,<title>Typing</title><input id=i value='old value'>"}),
            ))
            .await?;
        let input_index = input_index(&server).await?;

        let result = server
            .call_browser_tool(call(
                "browser_type",
                json!({"index": input_index, "text": "person@example.com"}),
            ))
            .await?;
        assert_eq!(text_content(&result), "Typed <email> into element 0");

        let value = server
            .actor()
            .evaluate("document.getElementById('i').value")
            .await?;
        assert_eq!(value, json!("person@example.com"));

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn typing_empty_text_clears_prefilled_input() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": "data:text/html,<title>Clear</title><input id=i value='old value'>"}),
            ))
            .await?;
        let input_index = input_index(&server).await?;

        let result = server
            .call_browser_tool(call(
                "browser_type",
                json!({"index": input_index, "text": ""}),
            ))
            .await?;
        assert_eq!(text_content(&result), "Typed '' into element 0");

        let value = server
            .actor()
            .evaluate("document.getElementById('i').value")
            .await?;
        assert_eq!(value, json!(""));

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn failing_navigate_returns_tool_error_not_transport_error() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();

        let result = server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": "http://127.0.0.1:9/unreachable"}),
            ))
            .await
            .expect("browser failure should be an MCP tool error, not ErrorData");

        assert!(
            result.is_error.unwrap_or(false),
            "recoverable browser failure should set is_error=true: {result:?}"
        );
        assert!(text_content(&result).contains("browser_navigate failed"));

        Ok(())
    }

    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn extract_content_posts_chat_request_and_returns_framed_answer() -> anyhow::Result<()> {
        let llm_server = MockHttpServer::spawn_json(
            serde_json::json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "choices": [
                    {
                        "index": 0,
                        "message": {"role": "assistant", "content": "Alpha is the useful fact."},
                        "finish_reason": "stop"
                    }
                ]
            })
            .to_string(),
        )
        .await?;
        let page_server = MockHttpServer::spawn_html(
            r#"
            <html>
              <head>
                <title>Extract Fixture</title>
                <style>.noise { display: none; }</style>
                <script>window.noise = true;</script>
              </head>
              <body>
                <nav>Navigation noise</nav>
                <main>
                  <h1>Alpha</h1>
                  <p>Alpha is the useful fact.</p>
                  <a href="/details">Details</a>
                </main>
              </body>
            </html>
            "#,
        )
        .await?;
        let _env = EnvGuard::set_many(&[
            ("OPENAI_API_KEY", "test-key"),
            ("OPENAI_BASE_URL", &llm_server.base_url()),
            ("BROWSER_USE_LLM_MODEL", "test-model"),
        ]);
        let server = BrowserUseMcpServer::new();

        server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": format!("{}/data", page_server.base_url())}),
            ))
            .await?;

        let result = server
            .call_browser_tool(call(
                "browser_extract_content",
                json!({"query": "What is Alpha?", "extract_links": true}),
            ))
            .await?;

        assert_eq!(
            text_content(&result),
            format!(
                "<url>{}/data</url>\n<query>What is Alpha?</query>\n<result>Alpha is the useful fact.</result>",
                page_server.base_url()
            )
        );
        let request = llm_server.received_request().await?;
        assert_eq!(request.path, "/chat/completions");
        assert_eq!(
            request.header("authorization"),
            Some("Bearer test-key"),
            "LLM client should use OPENAI_API_KEY as bearer auth"
        );
        assert!(
            !request
                .header("user-agent")
                .unwrap_or_default()
                .contains("OpenAI"),
            "default reqwest user-agent must not pretend to be OpenAI"
        );

        let body: Value = serde_json::from_slice(&request.body)?;
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
        // Defaults to Python's 0.7 (stored as f32, so compare with tolerance).
        let temperature = body["temperature"]
            .as_f64()
            .expect("temperature should default to a number");
        assert!(
            (temperature - 0.7).abs() < 1e-6,
            "temperature should default to 0.7, got {temperature}"
        );
        let user_prompt = body["messages"][1]["content"]
            .as_str()
            .expect("user message content should be a string");
        assert!(user_prompt.contains("<query>\nWhat is Alpha?\n</query>"));
        assert!(user_prompt.contains("<webpage_content>"));
        assert!(user_prompt.contains("Alpha is the useful fact."));
        assert!(user_prompt.contains("[Details]("));
        assert!(!user_prompt.contains("Navigation noise"));
        assert!(!user_prompt.contains("window.noise"));

        Ok(())
    }

    #[cfg(feature = "live-chrome")]
    #[tokio::test]
    #[cfg(feature = "live-chrome")]
    async fn navigate_enforces_allowed_domains_policy() -> anyhow::Result<()> {
        let server = BrowserUseMcpServer::new();
        server
            .actor()
            .set_policy(bu_actor::BrowserUrlPolicy {
                allowed_domains: vec!["example.com".to_owned()],
                ..Default::default()
            })
            .await?;

        // data: URLs are always allowed.
        let allowed = server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": data_url("<title>ok</title>")}),
            ))
            .await?;
        assert!(
            !allowed.is_error.unwrap_or(false),
            "data URL must be allowed under the policy: {allowed:?}"
        );

        // An off-allowlist URL is blocked before any navigation happens.
        let blocked = server
            .call_browser_tool(call(
                "browser_navigate",
                json!({"url": "https://evil.example.org/"}),
            ))
            .await?;
        assert!(
            blocked.is_error.unwrap_or(false),
            "off-domain navigation must be blocked: {blocked:?}"
        );
        assert!(
            text_content(&blocked).contains("security policy"),
            "block message should mention the policy: {blocked:?}"
        );

        Ok(())
    }

    #[cfg(feature = "live-chrome")]
    fn call(name: &'static str, arguments: Value) -> CallToolRequestParams {
        let Value::Object(arguments) = arguments else {
            unreachable!("test arguments are object literals")
        };

        CallToolRequestParams::new(name).with_arguments(Map::from_iter(arguments))
    }

    #[cfg(feature = "live-chrome")]
    fn text_content(result: &rmcp::model::CallToolResult) -> &str {
        match result.content.first() {
            Some(rmcp::model::ContentBlock::Text(text)) => text.text.as_str(),
            other => panic!("expected first text content block, got {other:?}"),
        }
    }

    #[cfg(feature = "live-chrome")]
    fn indexed_element(elements: &[Value], tag: &str, text: &str) -> i64 {
        elements
            .iter()
            .find(|element| element["tag"] == tag && element["text"] == text)
            .and_then(|element| element["index"].as_i64())
            .unwrap_or_else(|| panic!("missing indexed {tag} element with text {text:?}"))
    }

    #[cfg(feature = "live-chrome")]
    fn has_element(elements: &[Value], tag: &str, text: &str) -> bool {
        elements
            .iter()
            .any(|element| element["tag"] == tag && element["text"] == text)
    }

    #[cfg(feature = "live-chrome")]
    fn data_url(html: &str) -> String {
        format!("data:text/html;base64,{}", BASE64_STANDARD.encode(html))
    }

    #[cfg(feature = "live-chrome")]
    async fn input_index(server: &BrowserUseMcpServer) -> anyhow::Result<i64> {
        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        Ok(indexed_element(
            state["interactive_elements"]
                .as_array()
                .expect("elements should be an array"),
            "input",
            "old value",
        ))
    }

    #[cfg(feature = "live-chrome")]
    struct EnvGuard {
        previous: Vec<(&'static str, Option<String>)>,
    }

    #[cfg(feature = "live-chrome")]
    impl EnvGuard {
        fn set_many(values: &[(&'static str, &str)]) -> Self {
            let previous = values
                .iter()
                .map(|(key, value)| {
                    let previous = std::env::var(key).ok();
                    std::env::set_var(key, value);
                    (*key, previous)
                })
                .collect();
            Self { previous }
        }
    }

    #[cfg(feature = "live-chrome")]
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, previous) in self.previous.drain(..) {
                if let Some(value) = previous {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    #[cfg(feature = "live-chrome")]
    #[derive(Debug)]
    struct RecordedRequest {
        path: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    #[cfg(feature = "live-chrome")]
    impl RecordedRequest {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }
    }

    #[cfg(feature = "live-chrome")]
    struct MockHttpServer {
        address: std::net::SocketAddr,
        request: tokio::sync::oneshot::Receiver<RecordedRequest>,
    }

    #[cfg(feature = "live-chrome")]
    impl MockHttpServer {
        async fn spawn_json(response_body: String) -> anyhow::Result<Self> {
            Self::spawn("application/json", response_body).await
        }

        async fn spawn_html(response_body: &str) -> anyhow::Result<Self> {
            Self::spawn("text/html; charset=utf-8", response_body.to_owned()).await
        }

        async fn spawn(content_type: &'static str, response_body: String) -> anyhow::Result<Self> {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let (tx, request) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let Ok(recorded) = read_http_request(&mut stream).await else {
                    return;
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{response_body}",
                    response_body.len()
                );
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
                let _ = tx.send(recorded);
            });

            Ok(Self { address, request })
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.address)
        }

        async fn received_request(self) -> anyhow::Result<RecordedRequest> {
            Ok(self.request.await?)
        }
    }

    #[cfg(feature = "live-chrome")]
    async fn read_http_request(
        stream: &mut tokio::net::TcpStream,
    ) -> anyhow::Result<RecordedRequest> {
        let mut buffer = Vec::new();
        let mut scratch = [0_u8; 1024];
        let header_end = loop {
            let read = tokio::io::AsyncReadExt::read(stream, &mut scratch).await?;
            if read == 0 {
                anyhow::bail!("connection closed before request headers");
            }
            buffer.extend_from_slice(&scratch[..read]);
            if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };

        let headers_text = std::str::from_utf8(&buffer[..header_end])?;
        let mut lines = headers_text.split("\r\n");
        let request_line = lines
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing request line"))?;
        let path = request_line
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("missing request path"))?
            .to_owned();
        let headers = lines
            .filter(|line| !line.is_empty())
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_owned(), value.trim().to_owned()))
            })
            .collect::<Vec<_>>();
        let content_length = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.parse::<usize>().ok())
            .unwrap_or(0);

        let mut body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let read = tokio::io::AsyncReadExt::read(stream, &mut scratch).await?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&scratch[..read]);
        }
        body.truncate(content_length);

        Ok(RecordedRequest {
            path,
            headers,
            body,
        })
    }
}
