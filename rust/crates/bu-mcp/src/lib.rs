//! Minimal MCP server for the Rust browser-use rewrite.

use std::{
    future::{self, Future},
    sync::Arc,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bu_actor::{ActorHandle, ClickOutcome};
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
            "browser_screenshot" => self.screenshot(request.arguments).await,
            "browser_scroll" => self.scroll(request.arguments).await,
            "browser_go_back" => self.go_back().await,
            "browser_list_tabs" => self.list_tabs().await,
            "browser_switch_tab" => self.switch_tab(request.arguments).await,
            "browser_close_tab" => self.close_tab(request.arguments).await,
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
        let snapshot = self
            .actor
            .get_state(include_screenshot)
            .await
            .map_err(browser_error("browser_get_state failed"))?;
        let elements = snapshot
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

        let mut payload = json!({
            "url": snapshot.page.url,
            "title": snapshot.page.title,
            "elements": elements,
            "tabs": tabs
        });
        if let Some(screenshot) = snapshot.screenshot {
            payload["screenshot"] = json!({
                "mime_type": "image/png",
                "size_bytes": screenshot.len(),
                "data": BASE64_STANDARD.encode(screenshot)
            });
        }

        Ok(CallToolResult::structured(payload))
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
        if coordinate_x.is_some() || coordinate_y.is_some() {
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
        let html = self
            .actor
            .get_html(selector.map(str::to_owned))
            .await
            .map_err(browser_error("browser_get_html failed"))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(html)]))
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

fn browser_error(message: &'static str) -> impl FnOnce(anyhow::Error) -> ErrorData {
    move |error| {
        ErrorData::new(
            ErrorCode::INTERNAL_ERROR,
            message,
            Some(json!({ "error": error.to_string() })),
        )
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

fn optional_str<'a>(arguments: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a str> {
    arguments
        .and_then(|args| args.get(key))
        .and_then(Value::as_str)
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

/// Returns the 14 low-level browser-use tools exposed by the MVP server.
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
    use rmcp::model::CallToolRequestParams;
    #[cfg(feature = "live-chrome")]
    use serde_json::{json, Map, Value};
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
    fn tools_list_returns_14_low_level_tools() {
        let tools = low_level_tools();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();

        assert_eq!(tools.len(), 14);
        assert_eq!(
            names,
            [
                "browser_navigate",
                "browser_click",
                "browser_type",
                "browser_get_state",
                "browser_get_html",
                "browser_screenshot",
                "browser_scroll",
                "browser_go_back",
                "browser_list_tabs",
                "browser_switch_tab",
                "browser_close_tab",
                "browser_list_sessions",
                "browser_close_session",
                "browser_close_all",
            ]
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
        let elements = state["elements"]
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
                state["elements"].as_array().expect("elements should be an array"),
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
            state["elements"]
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
    async fn input_index(server: &BrowserUseMcpServer) -> anyhow::Result<i64> {
        let state = server
            .call_browser_tool(call("browser_get_state", json!({})))
            .await?
            .structured_content
            .expect("browser_get_state should return structured JSON");
        Ok(indexed_element(
            state["elements"]
                .as_array()
                .expect("elements should be an array"),
            "input",
            "old value",
        ))
    }
}
