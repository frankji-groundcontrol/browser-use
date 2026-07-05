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
async fn selector_map_detects_modern_interactive_elements_and_filters_hidden() -> anyhow::Result<()>
{
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
async fn selector_map_keeps_one_index_for_button_wrapping_icon_and_text() -> anyhow::Result<()> {
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
async fn bbox_filter_is_hierarchy_aware_not_all_pairs() -> anyhow::Result<()> {
    // A Bootstrap "stretched-link": an <a> geometrically covers the whole
    // card, but a sibling <button> is NOT its DOM descendant, so the button
    // must be kept (all-pairs geometry would wrongly drop it).
    let server = BrowserUseMcpServer::new();
    let html = r#"
            <title>Stretched Link</title>
            <div style="position:relative;width:300px;height:200px">
              <button style="position:absolute;left:20px;top:20px;width:120px;height:36px">Primary CTA</button>
              <a href="/card" style="position:absolute;left:0;top:0;width:300px;height:200px"></a>
            </div>
        "#;
    server
        .call_browser_tool(call("browser_navigate", json!({"url": data_url(html)})))
        .await?;
    let state = server
        .call_browser_tool(call("browser_get_state", json!({})))
        .await?
        .structured_content
        .expect("structured JSON");
    let elements = state["interactive_elements"].as_array().expect("array");
    assert!(
        has_element(elements, "button", "Primary CTA"),
        "sibling button must NOT be collapsed by a stretched-link sibling: {elements:?}"
    );
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn bbox_filter_keeps_propagating_child_in_link() -> anyhow::Result<()> {
    // A <button> nested in an <a> is itself a propagating element, so Python's
    // exception rule keeps it indexed.
    let server = BrowserUseMcpServer::new();
    let html = r#"
            <title>Nested Button</title>
            <a href="/product" style="display:inline-block;width:200px;height:50px">
              <button style="width:180px;height:30px;margin:10px">Add to cart</button>
            </a>
        "#;
    server
        .call_browser_tool(call("browser_navigate", json!({"url": data_url(html)})))
        .await?;
    let state = server
        .call_browser_tool(call("browser_get_state", json!({})))
        .await?
        .structured_content
        .expect("structured JSON");
    let elements = state["interactive_elements"].as_array().expect("array");
    assert!(
        has_element(elements, "button", "Add to cart"),
        "a propagating button nested in a link must stay indexed: {elements:?}"
    );
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn bbox_filter_does_not_propagate_role_link() -> anyhow::Result<()> {
    // <div role="link"> is intentionally NOT a propagating element (Python
    // has it commented out), so its tabbable child stays indexed.
    let server = BrowserUseMcpServer::new();
    let html = r#"
            <title>Role Link</title>
            <div role="link" style="display:block;width:200px;height:40px">
              <span tabindex="0" style="display:block;width:100%;height:100%">Details</span>
            </div>
        "#;
    server
        .call_browser_tool(call("browser_navigate", json!({"url": data_url(html)})))
        .await?;
    let state = server
        .call_browser_tool(call("browser_get_state", json!({})))
        .await?
        .structured_content
        .expect("structured JSON");
    let elements = state["interactive_elements"].as_array().expect("array");
    assert!(
        has_element(elements, "span", "Details"),
        "child of div[role=link] must not be collapsed: {elements:?}"
    );
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn paint_order_drops_element_under_semi_transparent_overlay() -> anyhow::Result<()> {
    // opacity 0.85 (>= 0.8) + a non-transparent background is an occluder, so a
    // fully-covered button is dropped (Python's threshold, not 0.9).
    let server = BrowserUseMcpServer::new();
    let html = r#"
            <title>Semi Overlay</title>
            <body style="margin:0">
              <button style="position:absolute;left:0;top:0;width:120px;height:40px;z-index:1">Buy</button>
              <div style="position:absolute;left:0;top:0;width:120px;height:40px;background:#000;opacity:0.85;z-index:9"></div>
            </body>
        "#;
    server
        .call_browser_tool(call("browser_navigate", json!({"url": data_url(html)})))
        .await?;
    let state = server
        .call_browser_tool(call("browser_get_state", json!({})))
        .await?
        .structured_content
        .expect("structured JSON");
    let elements = state["interactive_elements"].as_array().expect("array");
    assert!(
        !has_element(elements, "button", "Buy"),
        "button under a semi-transparent (opacity 0.85) overlay must be dropped: {elements:?}"
    );
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn indexed_click_after_scroll_uses_viewport_normalized_coordinates() -> anyhow::Result<()> {
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

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn guard_resets_active_page_that_became_disallowed() -> anyhow::Result<()> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
        time::Duration,
    };

    // A local HTTP server so the active tab lands on a real (non-data) URL
    // that a policy can then disallow — simulating a page reached by a
    // click/JS navigation that bypassed the navigate pre-check.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}/", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let server = thread::spawn(move || {
        while !stop_thread.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                    let mut buffer = [0u8; 1024];
                    let _ = stream.read(&mut buffer);
                    let body = "<title>local</title><h1>local page</h1>";
                    let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                }
                Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let mcp = BrowserUseMcpServer::new();
    // No policy yet: navigate to the local page (allowed) so it becomes active.
    mcp.call_browser_tool(call("browser_navigate", json!({ "url": url })))
        .await?;

    // Now restrict to example.com — the active 127.0.0.1 page is disallowed.
    mcp.actor()
        .set_policy(bu_actor::BrowserUrlPolicy {
            allowed_domains: vec!["example.com".to_owned()],
            ..Default::default()
        })
        .await?;

    // get_state's guard must reset the now-disallowed active page to about:blank.
    let state = mcp
        .call_browser_tool(call("browser_get_state", json!({})))
        .await?
        .structured_content
        .expect("browser_get_state should return structured JSON");
    assert_eq!(
        state["url"], "about:blank",
        "guard_active_url must reset a disallowed active page: {state:?}"
    );

    stop.store(true, Ordering::Relaxed);
    let _ = server.join();
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
async fn read_http_request(stream: &mut tokio::net::TcpStream) -> anyhow::Result<RecordedRequest> {
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
