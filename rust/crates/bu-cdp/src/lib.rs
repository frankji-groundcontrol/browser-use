//! Thin Chromium DevTools Protocol wrapper for the first Rust browser tools.

use std::{
    env,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Context, Result};
use chromiumoxide::{
    browser::{Browser, BrowserConfig},
    page::Page,
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

/// A launched Chromium session.
#[derive(Debug)]
pub struct BrowserSession {
    browser: Arc<Mutex<Browser>>,
    handler_task: JoinHandle<()>,
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

        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(error) = event {
                    eprintln!("chromiumoxide handler error: {error}");
                }
            }
        });

        Ok(Self {
            browser: Arc::new(Mutex::new(browser)),
            handler_task,
            user_data_dir,
        })
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
        self.page
            .goto(url)
            .await
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
}
