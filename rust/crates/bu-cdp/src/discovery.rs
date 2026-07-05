//! Headless flag, Chromium executable discovery, and unique user-data-dir helpers.

use std::{
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};

pub(crate) fn headless_from_env() -> bool {
    match env::var("BROWSER_USE_HEADLESS") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

pub(crate) fn chromium_path_from_env() -> Option<PathBuf> {
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

pub(crate) fn find_playwright_chromium() -> Option<PathBuf> {
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

pub(crate) fn unique_user_data_dir() -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_nanos();
    let path = env::temp_dir().join(format!("browser-use-rs-chromium-{nanos}"));
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create Chromium user data dir {}", path.display()))?;
    Ok(path)
}
