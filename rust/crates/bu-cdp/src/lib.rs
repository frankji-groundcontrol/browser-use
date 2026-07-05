//! Thin Chromium DevTools Protocol wrapper for the first Rust browser tools.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{anyhow, Context, Result};
use chromiumoxide::{
    browser::{Browser, BrowserConfig},
    cdp::browser_protocol::{
        accessibility::GetFullAxTreeParams,
        dom::{BackendNodeId, FocusParams, GetDocumentParams, ResolveNodeParams},
        dom_debugger::GetEventListenersParams,
        dom_snapshot::CaptureSnapshotParams,
        input::{
            DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams,
            DispatchMouseEventType, InsertTextParams, MouseButton,
        },
        page::CaptureScreenshotFormat,
    },
    cdp::js_protocol::runtime::ReleaseObjectParams,
    page::Page,
    page::ScreenshotParams,
};
use futures_util::StreamExt;
use tokio::{sync::Mutex, task::JoinHandle};

mod discovery;
mod dom;
mod geometry;
mod security;

pub use security::UrlPolicy;

/// Above this many visible nodes, skip the per-node JS click-listener probe in
/// `selector_map` to keep `get_state` responsive on very large pages.
const MAX_LISTENER_PROBE_NODES: usize = 500;

use discovery::{
    chromium_path_from_env, find_playwright_chromium, headless_from_env, unique_user_data_dir,
};
use dom::{
    apply_bounding_box_containment_filter, apply_paint_order_occlusion_filter,
    collect_enhanced_dom_nodes, collect_interactive_elements, is_click_like_event, merge_ax_tree,
    merge_snapshot, visible_backend_node_ids, REQUIRED_COMPUTED_STYLES,
};

/// Browser launch options used by the thin CDP session wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserLaunchOptions {
    /// Whether Chromium should run headless.
    pub headless: bool,
    /// Optional Chromium executable path. If omitted, Browser Use discovery is used.
    pub executable_path: Option<PathBuf>,
}

impl BrowserLaunchOptions {
    /// Builds launch options from Browser Use environment variables.
    pub fn from_env() -> Self {
        Self {
            headless: headless_from_env(),
            executable_path: chromium_path_from_env(),
        }
    }
}

impl Default for BrowserLaunchOptions {
    fn default() -> Self {
        Self {
            headless: true,
            executable_path: None,
        }
    }
}

/// Current page metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageState {
    /// Current page URL.
    pub url: String,
    /// Current page title.
    pub title: String,
}

/// Metadata for an open browser tab/page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabInfo {
    /// Browser Use short tab id, matching the Python MCP server convention.
    pub id: String,
    /// Full Chromium target id.
    pub target_id: String,
    /// Current page URL.
    pub url: String,
    /// Current page title.
    pub title: String,
    /// Whether this tab is the active MCP tab.
    pub active: bool,
}

/// One interactive element in the current live DOM selector map.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectorMapElement {
    /// Stable only for the current selector-map snapshot.
    pub index: usize,
    /// Chromium backend node id used for follow-up DOM operations.
    pub backend_node_id: BackendNodeId,
    /// Lowercase tag name.
    pub tag: String,
    /// Short label or visible descendant text.
    pub text: String,
    /// Raw href attribute for anchor-like elements, if present.
    pub href: Option<String>,
    /// Center X coordinate in CSS pixels relative to the viewport.
    pub x: f64,
    /// Center Y coordinate in CSS pixels relative to the viewport.
    pub y: f64,
}

impl SelectorMapElement {
    /// Returns the raw CDP backend node id.
    pub fn backend_node_id_value(&self) -> i64 {
        *self.backend_node_id.inner()
    }
}

/// A launched Chromium session.
#[derive(Debug)]
pub struct BrowserSession {
    browser: Arc<Mutex<Browser>>,
    handler_task: JoinHandle<()>,
    healthy: Arc<AtomicBool>,
    user_data_dir: PathBuf,
}

impl BrowserSession {
    /// Launches Chromium with default headless options.
    pub async fn launch_headless() -> Result<Self> {
        Self::launch_with_options(BrowserLaunchOptions::default()).await
    }

    /// Launches Chromium using `BROWSER_USE_HEADLESS` and executable discovery.
    pub async fn launch_from_env() -> Result<Self> {
        Self::launch_with_options(BrowserLaunchOptions::from_env()).await
    }

    /// Launches Chromium using explicit options.
    pub async fn launch_with_options(options: BrowserLaunchOptions) -> Result<Self> {
        let executable_path = options
            .executable_path
            .or_else(find_playwright_chromium)
            .context("could not find Chromium executable")?;

        let user_data_dir = unique_user_data_dir()?;

        let mut config = BrowserConfig::builder()
            .chrome_executable(executable_path)
            .user_data_dir(&user_data_dir)
            .no_sandbox()
            .arg("--disable-dev-shm-usage");

        if !options.headless {
            config = config.with_head();
        }

        let (browser, mut handler) = Browser::launch(
            config
                .build()
                .map_err(|err| anyhow!("failed to build Chromium config: {err}"))?,
        )
        .await
        .context("failed to launch Chromium")?;

        let healthy = Arc::new(AtomicBool::new(true));
        let handler_healthy = healthy.clone();
        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(err) = event {
                    if matches!(&err, chromiumoxide::error::CdpError::Serde(_)) {
                        continue;
                    }
                    tracing::warn!(%err, "chromiumoxide handler error");
                }
            }
            handler_healthy.store(false, Ordering::SeqCst);
            tracing::warn!("chromiumoxide handler ended; browser is unavailable");
        });

        Ok(Self {
            browser: Arc::new(Mutex::new(browser)),
            handler_task,
            healthy,
            user_data_dir,
        })
    }

    /// Returns whether the Chromium handler task still reports a live browser.
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    /// Opens a new page.
    pub async fn new_page(&self) -> Result<BrowserPage> {
        let page = self
            .browser
            .lock()
            .await
            .new_page("about:blank")
            .await
            .context("failed to create Chromium page")?;

        Ok(BrowserPage { page })
    }

    /// Returns metadata for all open pages.
    /// Returns Chromium's first existing page, if any — so callers adopt the
    /// browser's initial tab instead of opening a redundant second one.
    pub async fn first_page(&self) -> Result<Option<BrowserPage>> {
        let pages = self
            .browser
            .lock()
            .await
            .pages()
            .await
            .context("failed to list Chromium pages")?;
        Ok(pages.into_iter().next().map(|page| BrowserPage { page }))
    }

    /// Returns the browser's primary page, waiting briefly for Chromium's initial
    /// page to register before creating one — avoids a redundant second tab from a
    /// launch-time race where `pages()` is momentarily empty.
    pub async fn primary_page(&self) -> Result<BrowserPage> {
        for _ in 0..40 {
            if let Some(page) = self.first_page().await? {
                return Ok(page);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        self.new_page().await
    }

    pub async fn tabs(&self, active_page: Option<&BrowserPage>) -> Result<Vec<TabInfo>> {
        let pages = self
            .browser
            .lock()
            .await
            .pages()
            .await
            .context("failed to list Chromium pages")?;
        let active_target_id = active_page.map(BrowserPage::target_id);

        let mut tabs = Vec::with_capacity(pages.len());
        for page in pages {
            let browser_page = BrowserPage { page };
            // A page that is mid-close/detached (or chrome:// mid-nav) can fail its
            // state read with "receiver is gone"; skip it rather than failing the
            // whole listing. (Review item 2.5 — brittle tab listing.)
            let Ok(state) = browser_page.state().await else {
                continue;
            };
            let target_id = browser_page.target_id();
            tabs.push(TabInfo {
                id: short_tab_id(&target_id),
                active: active_target_id.as_ref() == Some(&target_id),
                target_id,
                url: state.url,
                title: state.title,
            });
        }

        Ok(tabs)
    }

    /// Activates and returns a page selected by short id, full target id, or index.
    pub async fn switch_tab(&self, tab_ref: &str) -> Result<BrowserPage> {
        let page = self.resolve_tab(tab_ref).await?;
        page.page
            .activate()
            .await
            .with_context(|| format!("failed to activate tab {tab_ref}"))?;
        Ok(page)
    }

    /// Closes a page selected by short id, full target id, or index.
    /// Closes the tab and returns its full target id (so the caller can tell
    /// whether the active tab was the one closed).
    pub async fn close_tab(&self, tab_ref: &str) -> Result<String> {
        let page = self.resolve_tab(tab_ref).await?;
        let target_id = page.target_id();
        page.page
            .close()
            .await
            .with_context(|| format!("failed to close tab {tab_ref}"))?;
        Ok(target_id)
    }

    /// Closes the Chromium browser.
    pub async fn close(&self) -> Result<()> {
        self.browser
            .lock()
            .await
            .close()
            .await
            .context("failed to close Chromium browser")?;
        Ok(())
    }

    async fn resolve_tab(&self, tab_ref: &str) -> Result<BrowserPage> {
        let pages = self
            .browser
            .lock()
            .await
            .pages()
            .await
            .context("failed to list Chromium pages")?;
        let page =
            page_by_ref(pages, tab_ref).with_context(|| format!("tab {tab_ref} not found"))?;

        Ok(BrowserPage { page })
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        self.handler_task.abort();
        let _ = fs::remove_dir_all(&self.user_data_dir);
    }
}

/// Thin wrapper around a Chromium page.
#[derive(Debug, Clone)]
pub struct BrowserPage {
    page: Page,
}

impl BrowserPage {
    /// Navigates this page to `url`.
    pub async fn navigate(&self, url: &str) -> Result<()> {
        // Bound navigation so a stalled/streaming page cannot hang the actor.
        tokio::time::timeout(std::time::Duration::from_secs(30), self.page.goto(url))
            .await
            .with_context(|| format!("navigation to {url} timed out"))?
            .with_context(|| format!("failed to navigate to {url}"))?;
        Ok(())
    }

    /// Returns current URL and title.
    pub async fn state(&self) -> Result<PageState> {
        let url = self
            .page
            .url()
            .await
            .context("failed to read page URL")?
            .unwrap_or_default();
        let title = self
            .page
            .evaluate("document.title")
            .await
            .context("failed to read page title")?
            .into_value()
            .context("failed to decode page title")?;

        Ok(PageState { url, title })
    }

    /// Returns the current page HTML.
    pub async fn content(&self) -> Result<String> {
        self.page
            .content()
            .await
            .context("failed to read page content")
    }

    /// Returns full page HTML, or one selected element's outer HTML.
    pub async fn html(&self, selector: Option<&str>) -> Result<String> {
        if let Some(selector) = selector {
            return self
                .page
                .find_element(selector)
                .await
                .with_context(|| format!("no element found for selector: {selector}"))?
                .outer_html()
                .await
                .context("failed to read element outer HTML")?
                .with_context(|| format!("no element found for selector: {selector}"));
        }

        self.content().await
    }

    /// Captures a PNG screenshot.
    pub async fn screenshot_png(&self, full_page: bool) -> Result<Vec<u8>> {
        self.page
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .full_page(full_page)
                    .build(),
            )
            .await
            .context("failed to capture page screenshot")
    }

    /// Scrolls the page by one standard MCP increment.
    pub async fn scroll(&self, direction: &str) -> Result<()> {
        let amount = match direction {
            "up" => -500,
            "down" => 500,
            other => return Err(anyhow!("unsupported scroll direction: {other}")),
        };
        let script = format!("window.scrollBy(0, {amount})");
        self.page
            .evaluate(script)
            .await
            .context("failed to scroll page")?;
        Ok(())
    }

    /// Returns the current vertical scroll offset.
    pub async fn scroll_y(&self) -> Result<f64> {
        self.page
            .evaluate("window.scrollY")
            .await
            .context("failed to read scroll position")?
            .into_value()
            .context("failed to decode scroll position")
    }

    /// Returns the current horizontal scroll offset.
    pub async fn scroll_x(&self) -> Result<f64> {
        self.page
            .evaluate("window.scrollX")
            .await
            .context("failed to read scroll position")?
            .into_value()
            .context("failed to decode scroll position")
    }

    /// Navigates back in browser history.
    pub async fn go_back(&self) -> Result<()> {
        self.page
            .evaluate("window.history.back()")
            .await
            .context("failed to navigate back")?;
        // Back navigation may not fire a lifecycle event (SPA hash routes, data:
        // URLs, no history entry); wait briefly but never hang or hard-fail if it
        // doesn't. (Review item 1.6 — robust go_back.)
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.page.wait_for_navigation(),
        )
        .await;
        Ok(())
    }

    /// Returns this page's full Chromium target id.
    pub fn target_id(&self) -> String {
        self.page.target_id().as_ref().to_string()
    }

    /// Returns whether the current DOM has at least one match for `selector`.
    pub async fn query_selector_exists(&self, selector: &str) -> Result<bool> {
        let script = format!(
            "document.querySelector({selector}) !== null",
            selector = serde_json::to_string(selector)?
        );

        self.page
            .evaluate(script)
            .await
            .context("failed to query DOM selector")?
            .into_value()
            .context("failed to decode selector query result")
    }

    /// Evaluates JavaScript in the current page and decodes the result as JSON.
    #[cfg(feature = "live-chrome")]
    pub async fn evaluate_json(&self, script: &str) -> Result<serde_json::Value> {
        let result = self
            .page
            .evaluate(script)
            .await
            .with_context(|| format!("failed to evaluate script: {script}"))?;
        // A void/undefined result (e.g. calling a function that returns nothing)
        // has no remote value; treat it as JSON null rather than an error.
        Ok(result.into_value().unwrap_or(serde_json::Value::Null))
    }

    /// Builds a selector map from fused DOM, DOMSnapshot, and AX trees.
    pub async fn selector_map(&self) -> Result<Vec<SelectorMapElement>> {
        let dom_tree = self
            .page
            .execute(GetDocumentParams::builder().depth(-1).pierce(true).build())
            .await
            .context("failed to read flattened DOM")?
            .result
            .root;
        let snapshot = self
            .page
            .execute(
                CaptureSnapshotParams::builder()
                    .computed_styles(REQUIRED_COMPUTED_STYLES.iter().copied())
                    .include_paint_order(true)
                    .include_dom_rects(true)
                    .build()
                    .map_err(|error| anyhow!("failed to build DOM snapshot request: {error}"))?,
            )
            .await
            .context("failed to capture DOM snapshot")?;
        // The Accessibility domain must be enabled before getFullAXTree, and some
        // targets don't support it — enable best-effort and degrade gracefully:
        // AX roles refine detection, but JS listeners + tags/attrs still classify
        // elements, so a missing AX tree must not fail get_state.
        let _ = self
            .page
            .execute(chromiumoxide::cdp::browser_protocol::accessibility::EnableParams::default())
            .await;
        let ax_nodes = self
            .page
            .execute(GetFullAxTreeParams::builder().build())
            .await
            .map(|tree| tree.nodes.clone())
            .unwrap_or_else(|err| {
                tracing::warn!(%err, "accessibility tree unavailable; continuing without AX roles");
                Vec::new()
            });
        let scroll_x = self.scroll_x().await.unwrap_or(0.0);
        let scroll_y = self.scroll_y().await.unwrap_or(0.0);
        let mut enhanced_nodes = HashMap::new();
        collect_enhanced_dom_nodes(&dom_tree, &mut enhanced_nodes);
        merge_snapshot(&snapshot, scroll_x, scroll_y, &mut enhanced_nodes);
        merge_ax_tree(&ax_nodes, &mut enhanced_nodes);
        // The JS click-listener probe does per-node CDP round-trips, so on very
        // large pages it would stall get_state for tens of seconds. Skip it past
        // a threshold (Python similarly bails on huge DOMs); AX roles, interactive
        // tags, onclick, and tabindex still classify elements.
        let listener_probe_ids = visible_backend_node_ids(&enhanced_nodes);
        if listener_probe_ids.len() <= MAX_LISTENER_PROBE_NODES {
            let js_click_listener_backend_ids = self
                .js_click_listener_backend_ids(&listener_probe_ids)
                .await;
            for backend_node_id in js_click_listener_backend_ids {
                if let Some(node) = enhanced_nodes.get_mut(&backend_node_id) {
                    node.has_js_click_listener = true;
                }
            }
        } else {
            tracing::warn!(
                visible_nodes = listener_probe_ids.len(),
                threshold = MAX_LISTENER_PROBE_NODES,
                "skipping JS click-listener detection on a very large page; relying on AX/tag/attribute heuristics"
            );
        }

        let mut candidates = Vec::new();
        collect_interactive_elements(&dom_tree, &enhanced_nodes, &mut candidates);
        apply_paint_order_occlusion_filter(&mut candidates, &enhanced_nodes);
        apply_bounding_box_containment_filter(&dom_tree, &enhanced_nodes, &mut candidates);

        let elements = candidates
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| candidate.into_element(index))
            .collect();

        Ok(elements)
    }

    async fn js_click_listener_backend_ids(
        &self,
        backend_node_ids: &[BackendNodeId],
    ) -> HashSet<i64> {
        let mut ids = HashSet::new();

        for &backend_node_id in backend_node_ids {
            let Ok(resolved) = self
                .page
                .execute(
                    ResolveNodeParams::builder()
                        .backend_node_id(backend_node_id)
                        .object_group("browser-use-selector-map")
                        .build(),
                )
                .await
            else {
                continue;
            };

            let Some(object_id) = resolved.object.object_id.clone() else {
                continue;
            };

            let listeners = self
                .page
                .execute(
                    GetEventListenersParams::builder()
                        .object_id(object_id.clone())
                        .build()
                        .expect("event listener params are valid"),
                )
                .await;
            let _ = self.page.execute(ReleaseObjectParams::new(object_id)).await;

            let Ok(listeners) = listeners else {
                continue;
            };

            let mut has_click_listener = false;
            for listener in &listeners.listeners {
                if is_click_like_event(&listener.r#type) {
                    has_click_listener = true;
                    if let Some(listener_backend_node_id) = listener.backend_node_id {
                        ids.insert(*listener_backend_node_id.inner());
                    }
                }
            }

            if has_click_listener {
                ids.insert(*backend_node_id.inner());
            }
        }

        ids
    }

    fn element_for_backend_node_id_from_map(
        elements: Vec<SelectorMapElement>,
        backend_node_id: i64,
    ) -> Result<SelectorMapElement> {
        elements
            .into_iter()
            .find(|element| element.backend_node_id_value() == backend_node_id)
            .with_context(|| format!("backend node id {backend_node_id} not found or not visible"))
    }

    async fn current_element_by_backend_node_id(
        &self,
        backend_node_id: i64,
    ) -> Result<SelectorMapElement> {
        Self::element_for_backend_node_id_from_map(self.selector_map().await?, backend_node_id)
    }

    /// Clicks an element from the current selector map by index.
    pub async fn click_element(&self, index: usize) -> Result<()> {
        let element = self
            .selector_map()
            .await?
            .into_iter()
            .find(|element| element.index == index)
            .with_context(|| format!("interactive element index {index} not found"))?;

        self.click_backend_node_id(element.backend_node_id_value())
            .await
            .with_context(|| format!("failed to click interactive element index {index}"))?;
        Ok(())
    }

    /// Clicks an element by stable Chromium backend node id.
    pub async fn click_backend_node_id(&self, backend_node_id: i64) -> Result<()> {
        let element = self
            .current_element_by_backend_node_id(backend_node_id)
            .await?;
        self.click_coordinates(element.x, element.y).await?;
        Ok(())
    }

    /// Dispatches a synthetic mouse click at viewport coordinates.
    pub async fn click_coordinates(&self, x: f64, y: f64) -> Result<()> {
        self.dispatch_mouse_event(DispatchMouseEventType::MousePressed, x, y)
            .await?;
        self.dispatch_mouse_event(DispatchMouseEventType::MouseReleased, x, y)
            .await?;
        Ok(())
    }

    /// Focuses an indexed element and types text into it.
    pub async fn type_into_element(&self, index: usize, text: &str) -> Result<()> {
        let element = self
            .selector_map()
            .await?
            .into_iter()
            .find(|element| element.index == index)
            .with_context(|| format!("interactive element index {index} not found"))?;

        self.type_into_backend_node_id(element.backend_node_id_value(), text)
            .await
            .with_context(|| format!("failed to type into interactive element index {index}"))?;

        Ok(())
    }

    /// Focuses an element by stable Chromium backend node id and types text into it.
    pub async fn type_into_backend_node_id(&self, backend_node_id: i64, text: &str) -> Result<()> {
        let element = self
            .current_element_by_backend_node_id(backend_node_id)
            .await?;

        self.page
            .execute(
                FocusParams::builder()
                    .backend_node_id(element.backend_node_id)
                    .build(),
            )
            .await
            .with_context(|| format!("failed to focus backend node id {backend_node_id}"))?;

        if let Err(insert_error) = self.page.execute(InsertTextParams::new(text)).await {
            self.dispatch_text_as_key_events(text)
                .await
                .with_context(|| {
                    format!("failed to type text after insertText failed: {insert_error}")
                })?;
        }

        Ok(())
    }

    /// Clears an element by stable Chromium backend node id, dispatching input/change events.
    pub async fn clear_backend_node_id(&self, backend_node_id: i64) -> Result<()> {
        let element = self
            .current_element_by_backend_node_id(backend_node_id)
            .await?;

        self.page
            .execute(
                FocusParams::builder()
                    .backend_node_id(element.backend_node_id)
                    .build(),
            )
            .await
            .with_context(|| format!("failed to focus backend node id {backend_node_id}"))?;

        self.page
            .evaluate(
                r#"
                (() => {
                    const el = document.activeElement;
                    if (!el) return false;
                    if ('value' in el) {
                        el.value = '';
                    } else {
                        el.textContent = '';
                    }
                    el.dispatchEvent(new Event('input', { bubbles: true }));
                    el.dispatchEvent(new Event('change', { bubbles: true }));
                    return true;
                })()
                "#,
            )
            .await
            .context("failed to clear focused element")?;

        Ok(())
    }

    /// Resolves `href` as a browser URL using the page's current base URI.
    pub async fn resolve_url(&self, href: &str) -> Result<String> {
        let script = format!(
            "new URL({}, document.baseURI).href",
            serde_json::to_string(href)?
        );
        self.page
            .evaluate(script)
            .await
            .context("failed to resolve URL")?
            .into_value()
            .context("failed to decode resolved URL")
    }

    async fn dispatch_mouse_event(
        &self,
        event_type: DispatchMouseEventType,
        x: f64,
        y: f64,
    ) -> Result<()> {
        let buttons = if event_type == DispatchMouseEventType::MousePressed {
            1
        } else {
            0
        };
        self.page
            .execute(
                DispatchMouseEventParams::builder()
                    .r#type(event_type)
                    .x(x)
                    .y(y)
                    .button(MouseButton::Left)
                    .buttons(buttons)
                    .click_count(1)
                    .build()
                    .map_err(|error| anyhow!("failed to build mouse event: {error}"))?,
            )
            .await
            .context("failed to dispatch mouse event")?;
        Ok(())
    }

    async fn dispatch_text_as_key_events(&self, text: &str) -> Result<()> {
        for ch in text.chars() {
            let text = ch.to_string();
            self.page
                .execute(
                    DispatchKeyEventParams::builder()
                        .r#type(DispatchKeyEventType::Char)
                        .text(text.clone())
                        .unmodified_text(text)
                        .build()
                        .map_err(|error| anyhow!("failed to build key event: {error}"))?,
                )
                .await
                .context("failed to dispatch key event")?;
        }

        Ok(())
    }
}

fn short_tab_id(target_id: &str) -> String {
    target_id
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn page_by_ref(pages: Vec<Page>, tab_ref: &str) -> Option<Page> {
    // Match by full target id or the documented 4-char short id first, so a tab
    // whose short id happens to be numeric is never mistaken for a position.
    if let Some(page) = pages.iter().find(|page| {
        let target_id = page.target_id();
        let target_id = target_id.as_ref();
        target_id == tab_ref || short_tab_id(target_id) == tab_ref
    }) {
        return Some(page.clone());
    }

    // Fall back to a 0-based positional index only when no id matched.
    if let Ok(index) = tab_ref.parse::<usize>() {
        return pages.get(index).cloned();
    }

    None
}

#[cfg(all(test, feature = "live-chrome"))]
mod live_tests;
