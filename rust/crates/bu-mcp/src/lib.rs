//! Minimal MCP server for the Rust browser-use rewrite.

use std::{
    future::{self, Future},
    sync::Arc,
};

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

/// Minimal rmcp server implementation for the browser-use MCP surface.
#[derive(Debug, Clone, Default)]
pub struct BrowserUseMcpServer;

impl BrowserUseMcpServer {
    /// Creates a new browser-use MCP server.
    pub fn new() -> Self {
        Self
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

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        let tool_exists = low_level_tools()
            .iter()
            .any(|tool| tool.name == request.name);

        let result = if tool_exists {
            Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "{} is not implemented yet",
                request.name
            ))]))
        } else {
            Err(ErrorData::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("Unknown tool: {}", request.name),
                None,
            ))
        };

        future::ready(result)
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
    use super::low_level_tools;

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
}
