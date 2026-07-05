//! Thin Chromium DevTools Protocol wrapper for the first Rust browser tools.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use chromiumoxide::{
    browser::{Browser, BrowserConfig},
    cdp::browser_protocol::{
        dom::{BackendNodeId, FocusParams, GetBoxModelParams, GetDocumentParams, Node},
        input::{
            DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams,
            DispatchMouseEventType, InsertTextParams, MouseButton,
        },
        page::CaptureScreenshotFormat,
    },
    page::Page,
    page::ScreenshotParams,
};
use futures_util::StreamExt;
use tokio::{sync::Mutex, task::JoinHandle};

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

#[derive(Debug)]
struct SelectorCandidate {
    backend_node_id: BackendNodeId,
    tag: String,
    text: String,
    href: Option<String>,
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
    pub async fn close_tab(&self, tab_ref: &str) -> Result<()> {
        let page = self.resolve_tab(tab_ref).await?;
        page.page
            .close()
            .await
            .with_context(|| format!("failed to close tab {tab_ref}"))?;
        Ok(())
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

    /// Builds a first-cut selector map from Chromium's live flattened DOM.
    pub async fn selector_map(&self) -> Result<Vec<SelectorMapElement>> {
        let root = self
            .page
            .execute(GetDocumentParams::builder().depth(-1).pierce(true).build())
            .await
            .context("failed to read flattened DOM")?
            .result
            .root;

        let mut candidates = Vec::new();
        collect_interactive_candidates(&root, &mut candidates);

        let mut elements = Vec::new();
        for candidate in candidates {
            if let Some((x, y)) = self.box_center(candidate.backend_node_id).await? {
                elements.push(SelectorMapElement {
                    index: elements.len(),
                    backend_node_id: candidate.backend_node_id,
                    tag: candidate.tag,
                    text: candidate.text,
                    href: candidate.href,
                    x,
                    y,
                });
            }
        }

        Ok(elements)
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
        let element = self.element_for_backend_node_id(backend_node_id).await?;
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
        let element = self.element_for_backend_node_id(backend_node_id).await?;

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
        let element = self.element_for_backend_node_id(backend_node_id).await?;

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

    async fn element_for_backend_node_id(
        &self,
        backend_node_id: i64,
    ) -> Result<SelectorMapElement> {
        let backend_node_id = BackendNodeId::new(backend_node_id);
        let (x, y) = self.box_center(backend_node_id).await?.with_context(|| {
            format!(
                "backend node id {} not found or not visible",
                *backend_node_id.inner()
            )
        })?;

        Ok(SelectorMapElement {
            index: 0,
            backend_node_id,
            tag: String::new(),
            text: String::new(),
            href: None,
            x,
            y,
        })
    }

    async fn box_center(&self, backend_node_id: BackendNodeId) -> Result<Option<(f64, f64)>> {
        let box_model = match self
            .page
            .execute(
                GetBoxModelParams::builder()
                    .backend_node_id(backend_node_id)
                    .build(),
            )
            .await
        {
            Ok(response) => response.result.model,
            Err(_) => return Ok(None),
        };
        let border = box_model.border.inner();
        if border.len() < 8 || box_model.width <= 0 || box_model.height <= 0 {
            return Ok(None);
        }

        let x = (border[0] + border[2] + border[4] + border[6]) / 4.0;
        let y = (border[1] + border[3] + border[5] + border[7]) / 4.0;
        Ok(Some((x, y)))
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

fn collect_interactive_candidates(node: &Node, candidates: &mut Vec<SelectorCandidate>) {
    if is_interactive_node(node) {
        candidates.push(SelectorCandidate {
            backend_node_id: node.backend_node_id,
            tag: node_tag(node),
            text: short_text(node_label(node)),
            href: attr_value(node, "href").map(ToOwned::to_owned),
        });
    }

    for child in node.children.iter().flatten() {
        collect_interactive_candidates(child, candidates);
    }
    for shadow_root in node.shadow_roots.iter().flatten() {
        collect_interactive_candidates(shadow_root, candidates);
    }
    if let Some(content_document) = &node.content_document {
        collect_interactive_candidates(content_document, candidates);
    }
    if let Some(template_content) = &node.template_content {
        collect_interactive_candidates(template_content, candidates);
    }
}

fn is_interactive_node(node: &Node) -> bool {
    if node.node_type != 1 {
        return false;
    }

    matches!(
        node_tag(node).as_str(),
        "a" | "button" | "input" | "select" | "textarea"
    ) || attr_value(node, "role").is_some_and(|role| role.eq_ignore_ascii_case("button"))
        || has_attr(node, "onclick")
        || has_attr(node, "contenteditable")
}

fn node_tag(node: &Node) -> String {
    let tag = if node.local_name.is_empty() {
        &node.node_name
    } else {
        &node.local_name
    };
    tag.to_ascii_lowercase()
}

fn node_label(node: &Node) -> String {
    ["aria-label", "title", "placeholder", "value", "alt"]
        .into_iter()
        .filter_map(|name| attr_value(node, name))
        .find(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| descendant_text(node))
}

fn descendant_text(node: &Node) -> String {
    let mut parts = Vec::new();
    collect_text(node, &mut parts);
    parts.join(" ")
}

fn collect_text(node: &Node, parts: &mut Vec<String>) {
    if node.node_type == 3 {
        let text = node.node_value.trim();
        if !text.is_empty() {
            parts.push(text.to_owned());
        }
    }

    for child in node.children.iter().flatten() {
        collect_text(child, parts);
    }
    for shadow_root in node.shadow_roots.iter().flatten() {
        collect_text(shadow_root, parts);
    }
}

fn short_text(text: String) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 80;
    if compact.chars().count() <= MAX_CHARS {
        return compact;
    }

    compact.chars().take(MAX_CHARS).collect()
}

fn attr_value<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    node.attributes.as_ref()?.chunks_exact(2).find_map(|chunk| {
        chunk[0]
            .eq_ignore_ascii_case(name)
            .then_some(chunk[1].as_str())
    })
}

fn has_attr(node: &Node, name: &str) -> bool {
    attr_value(node, name).is_some()
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
    if let Ok(index) = tab_ref.parse::<usize>() {
        if let Some(page) = pages.get(index).cloned() {
            return Some(page);
        }
        if index > 0 {
            if let Some(page) = pages.get(index - 1).cloned() {
                return Some(page);
            }
        }
    }

    pages.into_iter().find(|page| {
        let target_id = page.target_id().as_ref();
        target_id == tab_ref || short_tab_id(target_id) == tab_ref
    })
}

fn headless_from_env() -> bool {
    match env::var("BROWSER_USE_HEADLESS") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

fn chromium_path_from_env() -> Option<PathBuf> {
    [
        "PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH",
        "PLAYWRIGHT_CHROME_EXECUTABLE_PATH",
        "CHROMIUM_PATH",
        "CHROME",
    ]
    .into_iter()
    .filter_map(|key| env::var_os(key).map(PathBuf::from))
    .find(|path| path.is_file())
}

fn find_playwright_chromium() -> Option<PathBuf> {
    playwright_roots()
        .into_iter()
        .flat_map(|root| chromium_candidates(&root))
        .filter(|path| path.is_file())
        .max()
}

fn playwright_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(path) = env::var_os("PLAYWRIGHT_BROWSERS_PATH").map(PathBuf::from) {
        roots.push(path);
    }

    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".cache").join("ms-playwright"));
    }

    roots
}

fn chromium_candidates(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("chromium-"))
        })
        .map(|path| path.join("chrome-linux64").join("chrome"))
        .collect()
}

fn unique_user_data_dir() -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_nanos();
    let path = env::temp_dir().join(format!("browser-use-rs-chromium-{nanos}"));
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create Chromium user data dir {}", path.display()))?;
    Ok(path)
}

#[cfg(all(test, feature = "live-chrome"))]
mod live_tests {
    use super::BrowserSession;

    #[tokio::test]
    async fn launches_chromium_and_reports_data_url_title_and_url() -> anyhow::Result<()> {
        let session = BrowserSession::launch_headless().await?;
        let page = session.new_page().await?;

        page.navigate("data:text/html,<title>Rust CDP</title><main>Hello</main>")
            .await?;

        let state = page.state().await?;
        assert_eq!(state.title, "Rust CDP");
        assert!(state.url.starts_with("data:text/html"));

        Ok(())
    }

    #[tokio::test]
    async fn page_helpers_return_html_screenshot_scroll_and_history() -> anyhow::Result<()> {
        let session = BrowserSession::launch_headless().await?;
        let page = session.new_page().await?;

        page.navigate(
            "data:text/html,<title>First</title><main id='app'><p>one</p></main><div style='height:2000px'></div>",
        )
        .await?;
        let html = page.html(Some("#app")).await?;
        assert_eq!(html, "<main id=\"app\"><p>one</p></main>");

        let screenshot = page.screenshot_png(false).await?;
        assert!(screenshot.starts_with(b"\x89PNG\r\n\x1a\n"));

        page.scroll("down").await?;
        let scroll_y = page.scroll_y().await?;
        assert!(scroll_y > 0.0);

        page.navigate("data:text/html,<title>Second</title><main>two</main>")
            .await?;
        page.go_back().await?;

        let state = page.state().await?;
        assert_eq!(state.title, "First");

        Ok(())
    }

    #[tokio::test]
    async fn manages_pages_as_tabs() -> anyhow::Result<()> {
        let session = BrowserSession::launch_headless().await?;
        let first = session.new_page().await?;
        first
            .navigate("data:text/html,<title>First Tab</title>")
            .await?;
        let second = session.new_page().await?;
        second
            .navigate("data:text/html,<title>Second Tab</title>")
            .await?;

        let tabs = session.tabs(Some(&first)).await?;
        assert!(tabs
            .iter()
            .any(|tab| tab.title == "First Tab" && tab.active));
        assert!(tabs
            .iter()
            .any(|tab| tab.title == "Second Tab" && !tab.active));

        let second_id = tabs
            .iter()
            .find(|tab| tab.title == "Second Tab")
            .expect("second tab should be listed")
            .id
            .clone();
        let switched = session.switch_tab(&second_id).await?;
        assert_eq!(switched.state().await?.title, "Second Tab");

        session.close_tab(&second_id).await?;
        let tabs = session.tabs(Some(&first)).await?;
        assert!(!tabs.iter().any(|tab| tab.title == "Second Tab"));

        Ok(())
    }
}
