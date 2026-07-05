//! Single-owner browser actor for serialized browser-use operations.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::{anyhow, Context, Result};
use bu_cdp::{BrowserPage, BrowserSession, PageState, SelectorMapElement, TabInfo, UrlPolicy};
use tokio::sync::{mpsc, oneshot};

pub use bu_cdp::UrlPolicy as BrowserUrlPolicy;

const SESSION_ID: &str = "default";
/// Backstop timeout for any single browser command, so a wedged renderer
/// (e.g. an `onclick` that spins forever) can't hang the whole actor.
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

type Reply<T> = oneshot::Sender<Result<T>>;

/// Browser state returned by the actor for `browser_get_state`.
#[derive(Debug)]
pub struct BrowserStateSnapshot {
    /// Current page metadata.
    pub page: PageState,
    /// Interactive elements from the latest selector-map snapshot.
    pub elements: Vec<SelectorMapElement>,
    /// Open browser tabs.
    pub tabs: Vec<TabInfo>,
    /// Optional PNG screenshot bytes.
    pub screenshot: Option<Vec<u8>>,
}

/// Handle used by callers to send serialized commands to the browser actor.
#[derive(Debug, Clone)]
pub struct ActorHandle {
    tx: mpsc::Sender<Command>,
}

/// Result of an index-based click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickOutcome {
    /// A normal element click was dispatched.
    Clicked,
    /// A link href was opened in a new tab.
    OpenedNewTab(String),
    /// `new_tab` was requested, but the element had no href.
    NewTabUnsupported,
}

impl ActorHandle {
    /// Spawns a browser actor that lazily launches Chromium on first browser command.
    pub fn spawn() -> Self {
        Self::spawn_with_observer(None)
    }

    /// Spawns a browser actor and increments `launch_counter` for each Chromium launch.
    pub fn spawn_with_launch_counter(launch_counter: Arc<AtomicUsize>) -> Self {
        Self::spawn_with_observer(Some(launch_counter))
    }

    fn spawn_with_observer(launch_counter: Option<Arc<AtomicUsize>>) -> Self {
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            BrowserActor::new(launch_counter).run(rx).await;
        });
        Self { tx }
    }

    /// Navigates the active page, optionally opening a new tab first.
    pub async fn navigate(&self, url: String, new_tab: bool) -> Result<()> {
        self.request(|reply| Command::Navigate {
            url,
            new_tab,
            reply,
        })
        .await
    }

    /// Replaces the URL access policy and returns the previous one, so callers
    /// (e.g. the agent's per-run `allowed_domains`) can scope an override and
    /// restore it afterward.
    pub async fn set_policy(&self, policy: UrlPolicy) -> Result<UrlPolicy> {
        self.request(|reply| Command::SetPolicy { policy, reply })
            .await
    }

    /// Returns the current URL access policy.
    pub async fn get_policy(&self) -> Result<UrlPolicy> {
        self.request(|reply| Command::GetPolicy { reply }).await
    }

    /// Returns current page state and updates the stable selector cache.
    pub async fn get_state(&self, include_screenshot: bool) -> Result<BrowserStateSnapshot> {
        self.request(|reply| Command::GetState {
            include_screenshot,
            reply,
        })
        .await
    }

    /// Returns current page metadata without rebuilding the selector cache.
    pub async fn page_state(&self) -> Result<PageState> {
        self.request(|reply| Command::PageState { reply }).await
    }

    /// Clicks an element by the index from the last selector snapshot.
    pub async fn click(&self, index: usize, new_tab: bool) -> Result<ClickOutcome> {
        self.request(|reply| Command::Click {
            index,
            new_tab,
            reply,
        })
        .await
    }

    /// Clicks at viewport coordinates without resolving an element.
    pub async fn click_coordinates(&self, x: f64, y: f64) -> Result<()> {
        self.request(|reply| Command::ClickCoordinates { x, y, reply })
            .await
    }

    /// Types into an element by the index from the last selector snapshot.
    pub async fn type_text(&self, index: usize, text: String) -> Result<()> {
        self.request(|reply| Command::Type { index, text, reply })
            .await
    }

    /// Scrolls the active page.
    pub async fn scroll(&self, direction: String) -> Result<()> {
        self.request(|reply| Command::Scroll { direction, reply })
            .await
    }

    /// Navigates back in the active page.
    pub async fn go_back(&self) -> Result<()> {
        self.request(|reply| Command::GoBack { reply }).await
    }

    /// Captures a screenshot of the active page.
    pub async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>> {
        self.request(|reply| Command::Screenshot { full_page, reply })
            .await
    }

    /// Returns HTML for the active page or a selected element.
    pub async fn get_html(&self, selector: Option<String>) -> Result<String> {
        self.request(|reply| Command::GetHtml { selector, reply })
            .await
    }

    /// Lists open tabs.
    pub async fn list_tabs(&self) -> Result<Vec<TabInfo>> {
        self.request(|reply| Command::ListTabs { reply }).await
    }

    /// Switches active tab.
    pub async fn switch_tab(&self, tab: String) -> Result<PageState> {
        self.request(|reply| Command::SwitchTab { tab, reply })
            .await
    }

    /// Closes a tab.
    pub async fn close_tab(&self, tab: String) -> Result<String> {
        self.request(|reply| Command::CloseTab { tab, reply }).await
    }

    /// Lists active sessions. Stage 1 keeps the MVP single default session.
    pub async fn list_sessions(&self) -> Result<Option<String>> {
        self.request(|reply| Command::ListSessions { reply }).await
    }

    /// Closes one session by id.
    pub async fn close_session(&self, id: String) -> Result<bool> {
        self.request(|reply| Command::CloseSession { id, reply })
            .await
    }

    /// Closes all sessions.
    pub async fn close_all(&self) -> Result<bool> {
        self.request(|reply| Command::CloseAll { reply }).await
    }

    /// Evaluates JavaScript in live Chrome tests without exposing an MCP tool.
    #[cfg(feature = "live-chrome")]
    pub async fn evaluate(&self, script: &str) -> Result<serde_json::Value> {
        self.request(|reply| Command::Evaluate {
            script: script.to_owned(),
            reply,
        })
        .await
    }

    async fn request<T>(&self, build: impl FnOnce(Reply<T>) -> Command) -> Result<T>
    where
        T: Send + 'static,
    {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(build(reply))
            .await
            .context("browser actor stopped")?;
        rx.await.context("browser actor dropped command")?
    }
}

enum Command {
    SetPolicy {
        policy: UrlPolicy,
        reply: Reply<UrlPolicy>,
    },
    GetPolicy {
        reply: Reply<UrlPolicy>,
    },
    Navigate {
        url: String,
        new_tab: bool,
        reply: Reply<()>,
    },
    GetState {
        include_screenshot: bool,
        reply: Reply<BrowserStateSnapshot>,
    },
    PageState {
        reply: Reply<PageState>,
    },
    Click {
        index: usize,
        new_tab: bool,
        reply: Reply<ClickOutcome>,
    },
    ClickCoordinates {
        x: f64,
        y: f64,
        reply: Reply<()>,
    },
    Type {
        index: usize,
        text: String,
        reply: Reply<()>,
    },
    Scroll {
        direction: String,
        reply: Reply<()>,
    },
    GoBack {
        reply: Reply<()>,
    },
    Screenshot {
        full_page: bool,
        reply: Reply<Vec<u8>>,
    },
    GetHtml {
        selector: Option<String>,
        reply: Reply<String>,
    },
    ListTabs {
        reply: Reply<Vec<TabInfo>>,
    },
    SwitchTab {
        tab: String,
        reply: Reply<PageState>,
    },
    CloseTab {
        tab: String,
        reply: Reply<String>,
    },
    ListSessions {
        reply: Reply<Option<String>>,
    },
    CloseSession {
        id: String,
        reply: Reply<bool>,
    },
    CloseAll {
        reply: Reply<bool>,
    },
    #[cfg(feature = "live-chrome")]
    Evaluate {
        script: String,
        reply: Reply<serde_json::Value>,
    },
}

struct BrowserActor {
    session: Option<BrowserSession>,
    page: Option<BrowserPage>,
    selector_cache: SelectorCache,
    launch_counter: Option<Arc<AtomicUsize>>,
    policy: UrlPolicy,
}

impl BrowserActor {
    fn new(launch_counter: Option<Arc<AtomicUsize>>) -> Self {
        Self {
            session: None,
            page: None,
            selector_cache: SelectorCache::default(),
            launch_counter,
            policy: UrlPolicy::from_env(),
        }
    }

    async fn run(mut self, mut rx: mpsc::Receiver<Command>) {
        while let Some(command) = rx.recv().await {
            // Cancelling the dispatch future on timeout drops its reply sender,
            // so the caller gets an error and the actor moves to the next command.
            if tokio::time::timeout(COMMAND_TIMEOUT, self.dispatch(command))
                .await
                .is_err()
            {
                tracing::warn!("browser command timed out; dropping it and continuing");
            }
        }
    }

    async fn dispatch(&mut self, command: Command) {
        match command {
            Command::SetPolicy { policy, reply } => {
                let previous = std::mem::replace(&mut self.policy, policy);
                let _ = reply.send(Ok(previous));
            }
            Command::GetPolicy { reply } => {
                let _ = reply.send(Ok(self.policy.clone()));
            }
            Command::Navigate {
                url,
                new_tab,
                reply,
            } => {
                let _ = reply.send(self.navigate(&url, new_tab).await);
            }
            Command::GetState {
                include_screenshot,
                reply,
            } => {
                let _ = reply.send(self.get_state(include_screenshot).await);
            }
            Command::PageState { reply } => {
                let result = match self.active_page().await {
                    Ok(page) => page.state().await,
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Command::Click {
                index,
                new_tab,
                reply,
            } => {
                let _ = reply.send(self.click(index, new_tab).await);
            }
            Command::ClickCoordinates { x, y, reply } => {
                let result = match self.active_page().await {
                    Ok(page) => page.click_coordinates(x, y).await,
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Command::Type { index, text, reply } => {
                let _ = reply.send(self.type_text(index, &text).await);
            }
            Command::Scroll { direction, reply } => {
                let result = match self.active_page().await {
                    Ok(page) => page.scroll(&direction).await,
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Command::GoBack { reply } => {
                let _ = reply.send(self.go_back().await);
            }
            Command::Screenshot { full_page, reply } => {
                let _ = reply.send(self.screenshot(full_page).await);
            }
            Command::GetHtml { selector, reply } => {
                let _ = reply.send(self.get_html(selector.as_deref()).await);
            }
            Command::ListTabs { reply } => {
                let _ = reply.send(self.tabs().await);
            }
            Command::SwitchTab { tab, reply } => {
                let _ = reply.send(self.switch_tab(&tab).await);
            }
            Command::CloseTab { tab, reply } => {
                let _ = reply.send(self.close_tab(&tab).await);
            }
            Command::ListSessions { reply } => {
                let _ = reply.send(self.list_sessions().await);
            }
            Command::CloseSession { id, reply } => {
                let _ = reply.send(self.close_session(&id).await);
            }
            Command::CloseAll { reply } => {
                let _ = reply.send(self.close_all().await);
            }
            #[cfg(feature = "live-chrome")]
            Command::Evaluate { script, reply } => {
                let _ = reply.send(self.evaluate(&script).await);
            }
        }
    }

    /// Navigates back and invalidates the selector cache (the document changed).
    async fn go_back(&mut self) -> Result<()> {
        let page = self.active_page().await?;
        page.go_back().await?;
        self.selector_cache.clear();
        Ok(())
    }

    async fn navigate(&mut self, url: &str, new_tab: bool) -> Result<()> {
        // Enforcement point #1: block disallowed targets before navigating.
        self.ensure_url_allowed(url)?;
        let page = if new_tab {
            self.new_active_page().await?
        } else {
            self.active_page().await?
        };
        page.navigate(url).await?;
        self.selector_cache.clear();
        // Enforcement point #2: catch redirects into a disallowed domain and
        // reset to about:blank (mirrors on_NavigationCompleteEvent).
        if !self.policy.is_unrestricted() {
            let landed = page.state().await?;
            if !self.policy.is_url_allowed(&landed.url) {
                let _ = page.navigate("about:blank").await;
                self.selector_cache.clear();
                return Err(anyhow!(
                    "navigation blocked by security policy: redirected to disallowed URL {}",
                    landed.url
                ));
            }
        }
        Ok(())
    }

    fn ensure_url_allowed(&self, url: &str) -> Result<()> {
        if !self.policy.is_url_allowed(url) {
            return Err(anyhow!(
                "navigation blocked by security policy: {url} is not in the allowed domains"
            ));
        }
        Ok(())
    }

    /// Enforcement point #4: catch a disallowed URL reached by ANY path (a DOM
    /// click that navigated, JS `window.open`/`location=`, a redirect) at the
    /// observation boundary, resetting it to about:blank so disallowed content is
    /// never returned. Best-effort; mirrors Python's on_NavigationCompleteEvent.
    async fn guard_active_url(&mut self) {
        if self.policy.is_unrestricted() || self.page.is_none() {
            return;
        }
        let Ok(page) = self.active_page().await else {
            return;
        };
        let Ok(state) = page.state().await else {
            return;
        };
        if !self.policy.is_url_allowed(&state.url) {
            tracing::warn!(url = %state.url, "resetting disallowed page to about:blank (security policy)");
            let _ = page.navigate("about:blank").await;
            self.selector_cache.clear();
        }
    }

    async fn get_state(&mut self, include_screenshot: bool) -> Result<BrowserStateSnapshot> {
        self.guard_active_url().await;
        let page = self.active_page().await?;
        let state = page.state().await?;
        let elements = page.selector_map().await?;
        self.selector_cache.replace(&elements);
        let tabs = self.tabs().await?;
        let screenshot = if include_screenshot {
            Some(page.screenshot_png(false).await?)
        } else {
            None
        };

        Ok(BrowserStateSnapshot {
            page: state,
            elements,
            tabs,
            screenshot,
        })
    }

    async fn click(&mut self, index: usize, new_tab: bool) -> Result<ClickOutcome> {
        let backend_node_id = self.backend_node_id_for_index(index).await?;
        let href = self.selector_cache.href_for_index(index);
        let page = self.active_page().await?;

        if new_tab {
            if let Some(href) = href {
                let url = page.resolve_url(&href).await?;
                // Enforcement point #3: block disallowed new-tab targets.
                self.ensure_url_allowed(&url)?;
                self.new_active_page().await?.navigate(&url).await?;
                self.selector_cache.clear();
                return Ok(ClickOutcome::OpenedNewTab(url));
            }

            page.click_backend_node_id(backend_node_id).await?;
            return Ok(ClickOutcome::NewTabUnsupported);
        }

        page.click_backend_node_id(backend_node_id).await?;
        Ok(ClickOutcome::Clicked)
    }

    async fn type_text(&mut self, index: usize, text: &str) -> Result<()> {
        let backend_node_id = self.backend_node_id_for_index(index).await?;
        let page = self.active_page().await?;
        page.clear_backend_node_id(backend_node_id).await?;
        if text.is_empty() {
            return Ok(());
        }
        page.type_into_backend_node_id(backend_node_id, text).await
    }

    async fn screenshot(&mut self, full_page: bool) -> Result<Vec<u8>> {
        self.guard_active_url().await;
        self.active_page().await?.screenshot_png(full_page).await
    }

    async fn get_html(&mut self, selector: Option<&str>) -> Result<String> {
        self.guard_active_url().await;
        let page = self.active_page().await?;
        if let Some(selector) = selector {
            let exists = page.query_selector_exists(selector).await?;
            if !exists {
                return Ok(format!("No element found for selector: {selector}"));
            }
        }
        page.html(selector).await
    }

    async fn tabs(&mut self) -> Result<Vec<TabInfo>> {
        let Some(session) = self.session.as_ref() else {
            return Ok(Vec::new());
        };
        session.tabs(self.page.as_ref()).await
    }

    async fn switch_tab(&mut self, tab: &str) -> Result<PageState> {
        let session = self.active_session().await?;
        let page = session.switch_tab(tab).await?;
        self.page = Some(page);
        self.selector_cache.clear();
        // Reset the tab if it (e.g. a JS-opened tab) is on a disallowed URL.
        self.guard_active_url().await;
        self.active_page().await?.state().await
    }

    async fn close_tab(&mut self, tab: &str) -> Result<String> {
        let active_target = self.page.as_ref().map(BrowserPage::target_id);
        let session = self.active_session().await?;
        let closed_target = session.close_tab(tab).await?;

        // Only re-point the active tab if the tab we closed WAS the active one
        // (exact target-id match, not a suffix collision).
        if active_target.as_deref() == Some(closed_target.as_str()) {
            self.page = session.switch_tab("0").await.ok();
        }

        self.selector_cache.clear();
        let current_url = match self.page.as_ref() {
            Some(page) => page
                .state()
                .await
                .map(|state| state.url)
                .unwrap_or_default(),
            None => String::new(),
        };
        Ok(current_url)
    }

    async fn list_sessions(&mut self) -> Result<Option<String>> {
        let Some(page) = self.page.as_ref() else {
            return Ok(None);
        };
        Ok(Some(
            page.state()
                .await
                .map(|state| state.url)
                .unwrap_or_default(),
        ))
    }

    async fn close_session(&mut self, id: &str) -> Result<bool> {
        if id != SESSION_ID {
            return Ok(false);
        }
        self.close_all().await
    }

    async fn close_all(&mut self) -> Result<bool> {
        let Some(session) = self.session.take() else {
            self.page = None;
            self.selector_cache.clear();
            return Ok(false);
        };
        self.page = None;
        self.selector_cache.clear();
        session.close().await?;
        Ok(true)
    }

    #[cfg(feature = "live-chrome")]
    async fn evaluate(&mut self, script: &str) -> Result<serde_json::Value> {
        self.active_page().await?.evaluate_json(script).await
    }

    async fn backend_node_id_for_index(&mut self, index: usize) -> Result<i64> {
        if let Some(backend_node_id) = self.selector_cache.backend_node_id_for_index(index) {
            return Ok(backend_node_id);
        }

        let elements = self.active_page().await?.selector_map().await?;
        self.selector_cache.replace(&elements);
        self.selector_cache
            .backend_node_id_for_index(index)
            .with_context(|| format!("interactive element index {index} not found"))
    }

    async fn active_page(&mut self) -> Result<BrowserPage> {
        if let Some(page) = self.page.clone() {
            return Ok(page);
        }
        // Adopt Chromium's existing initial page instead of opening a second tab.
        let session = self.active_session().await?;
        let page = session.primary_page().await?;
        self.page = Some(page.clone());
        self.selector_cache.clear();
        Ok(page)
    }

    async fn new_active_page(&mut self) -> Result<BrowserPage> {
        let session = self.active_session().await?;
        let page = session.new_page().await?;
        self.page = Some(page.clone());
        self.selector_cache.clear();
        Ok(page)
    }

    async fn active_session(&mut self) -> Result<&BrowserSession> {
        if self
            .session
            .as_ref()
            .is_some_and(BrowserSession::is_healthy)
        {
            return Ok(self.session.as_ref().expect("session checked above"));
        }

        if self.session.is_some() {
            tracing::warn!("browser session is unhealthy; dropping it before relaunch");
        }
        self.session = None;
        self.page = None;
        self.selector_cache.clear();

        if let Some(counter) = &self.launch_counter {
            counter.fetch_add(1, Ordering::SeqCst);
        }
        self.session = Some(BrowserSession::launch_from_env().await?);
        self.session
            .as_ref()
            .ok_or_else(|| anyhow!("browser session was not initialized"))
    }
}

#[derive(Default)]
struct SelectorCache {
    index_to_backend_node_id: HashMap<usize, i64>,
    by_backend_node_id: HashMap<i64, SelectorMapElement>,
}

impl SelectorCache {
    fn replace(&mut self, elements: &[SelectorMapElement]) {
        self.clear();
        for element in elements {
            let backend_node_id = element.backend_node_id_value();
            self.index_to_backend_node_id
                .insert(element.index, backend_node_id);
            self.by_backend_node_id
                .insert(backend_node_id, element.clone());
        }
    }

    fn backend_node_id_for_index(&self, index: usize) -> Option<i64> {
        self.index_to_backend_node_id
            .get(&index)
            .copied()
            .filter(|backend_node_id| self.by_backend_node_id.contains_key(backend_node_id))
    }

    fn href_for_index(&self, index: usize) -> Option<String> {
        let backend_node_id = self.backend_node_id_for_index(index)?;
        self.by_backend_node_id
            .get(&backend_node_id)
            .and_then(|element| element.href.clone())
    }

    fn clear(&mut self) {
        self.index_to_backend_node_id.clear();
        self.by_backend_node_id.clear();
    }
}
