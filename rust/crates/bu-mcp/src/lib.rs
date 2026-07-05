//! Minimal MCP server for the Rust browser-use rewrite.

use std::{
    future::{self, Future},
    sync::Arc,
};

use bu_cdp::{BrowserPage, BrowserSession};
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
use tokio::sync::Mutex;

/// Minimal rmcp server implementation for the browser-use MCP surface.
#[derive(Debug, Clone, Default)]
pub struct BrowserUseMcpServer {
    browser: Arc<Mutex<Option<SharedBrowser>>>,
}

#[derive(Debug)]
struct SharedBrowser {
    _session: BrowserSession,
    page: BrowserPage,
}

impl BrowserUseMcpServer {
    /// Creates a new browser-use MCP server.
    pub fn new() -> Self {
        Self::default()
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
            "browser_get_state" => self.get_state().await,
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

        let page = self.shared_page().await?;
        page.navigate(url)
            .await
            .map_err(browser_error("browser_navigate failed"))?;

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Navigated to {url}"
        ))]))
    }

    async fn get_state(&self) -> Result<CallToolResult, ErrorData> {
        let page = self.shared_page().await?;
        let state = page
            .state()
            .await
            .map_err(browser_error("browser_get_state failed"))?;

        let payload = json!({
            "url": state.url,
            "title": state.title,
            "tabs": [
                {
                    "id": "0001",
                    "url": state.url,
                    "title": state.title,
                    "active": true
                }
            ]
        });

        Ok(CallToolResult::structured(payload))
    }

    async fn shared_page(&self) -> Result<BrowserPage, ErrorData> {
        let mut browser = self.browser.lock().await;

        if let Some(shared) = browser.as_ref() {
            return Ok(shared.page.clone());
        }

        let session = BrowserSession::launch_from_env()
            .await
            .map_err(browser_error("failed to launch browser"))?;
        let page = session
            .new_page()
            .await
            .map_err(browser_error("failed to create browser page"))?;

        *browser = Some(SharedBrowser {
            _session: session,
            page: page.clone(),
        });

        Ok(page)
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
    use super::{low_level_tools, BrowserUseMcpServer};
    use rmcp::model::CallToolRequestParams;
    use serde_json::{json, Map, Value};

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

    fn call(name: &'static str, arguments: Value) -> CallToolRequestParams {
        let Value::Object(arguments) = arguments else {
            unreachable!("test arguments are object literals")
        };

        CallToolRequestParams::new(name).with_arguments(Map::from_iter(arguments))
    }
}
