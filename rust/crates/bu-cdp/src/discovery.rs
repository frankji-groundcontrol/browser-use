//! Headless flag, Chromium executable discovery, and unique user-data-dir helpers.

use std::{
    env, fs,
    fs::File,
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

/// Profile directory to reuse across runs, from `BROWSER_USE_USER_DATA_DIR`.
///
/// Unset means a throwaway profile (deleted on close), so logins never survive.
/// Point this at a stable directory to keep cookies: log in once with
/// `BROWSER_USE_HEADLESS=false`, and later headless runs start authenticated.
pub(crate) fn user_data_dir_from_env() -> Option<PathBuf> {
    parse_user_data_dir(env::var("BROWSER_USE_USER_DATA_DIR").ok())
}

/// DevTools endpoint of an already-running Chromium, from `BROWSER_USE_CDP_URL`.
///
/// When set, attach to that browser instead of launching one — it keeps whatever
/// profile and logins it already has. Takes precedence over the profile dir.
pub(crate) fn cdp_url_from_env() -> Option<String> {
    parse_cdp_url(env::var("BROWSER_USE_CDP_URL").ok())
}

/// Split from the env read so it is testable without mutating process globals.
fn parse_user_data_dir(raw: Option<String>) -> Option<PathBuf> {
    let raw = raw?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(expand_tilde(trimmed))
}

fn parse_cdp_url(raw: Option<String>) -> Option<String> {
    let raw = raw?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Expands a leading `~/`. MCP servers are configured with JSON env blocks, which
/// no shell ever expands, so a literal `~/...` would otherwise create a directory
/// named `~` in the cwd.
fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
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

const SCRATCH_PREFIX: &str = "browser-use-rs-chromium-";
const LOCK_FILE: &str = ".bu-owner.lock";

/// Creates a throwaway profile dir, returning it plus the lock file that marks it
/// as in use. Also sweeps profiles left behind by dead sessions.
///
/// Deleting on `Drop` alone cannot work: chromiumoxide only sets `kill_on_drop`
/// and lets the runtime reap Chromium in the background, so the profile is still
/// held when `Drop` runs — and a SIGKILLed process never runs `Drop` at all.
/// Measured result was 24 stale profiles / 390 MB. The lock is the fix: the OS
/// drops it when the owning process dies however it dies, so any profile whose
/// lock we can take is provably abandoned.
pub(crate) fn unique_user_data_dir() -> Result<(PathBuf, File)> {
    let root = env::temp_dir();
    sweep_abandoned_user_data_dirs(&root);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_nanos();
    let path = root.join(format!("{SCRATCH_PREFIX}{nanos}-{}", std::process::id()));
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create Chromium user data dir {}", path.display()))?;

    let lock = File::create(path.join(LOCK_FILE))
        .with_context(|| format!("failed to create profile lock in {}", path.display()))?;
    lock.try_lock()
        .with_context(|| format!("failed to lock profile dir {}", path.display()))?;
    Ok((path, lock))
}

/// Removes scratch profiles whose owning process is gone. Skips any directory
/// whose lock is still held — that is a live session, possibly another agent's.
fn sweep_abandoned_user_data_dirs(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(std::result::Result::ok) {
        let path = entry.path();
        let is_scratch = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(SCRATCH_PREFIX));
        if !is_scratch || !path.is_dir() {
            continue;
        }
        let lock_path = path.join(LOCK_FILE);
        if lock_path.exists() {
            // Openable AND lockable means nobody owns it any more.
            match File::open(&lock_path).map(|file| file.try_lock().is_ok()) {
                Ok(true) => {}
                _ => continue,
            }
        }
        // No lock file at all: left by a build predating the lock; also abandoned.
        let _ = fs::remove_dir_all(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_and_unset_env_values_mean_no_override() {
        // A user who clears the value in their MCP config expects the default
        // (throwaway profile / launch our own browser), not a path named "".
        assert_eq!(parse_user_data_dir(None), None);
        assert_eq!(parse_user_data_dir(Some("   ".to_owned())), None);
        assert_eq!(parse_cdp_url(None), None);
        assert_eq!(parse_cdp_url(Some(String::new())), None);
    }

    #[test]
    fn values_are_trimmed_and_tilde_is_expanded() {
        // MCP env blocks are JSON: no shell expands `~`, so we must.
        let home = env::var("HOME").expect("HOME set in tests");
        assert_eq!(
            parse_user_data_dir(Some("~/profiles/work ".to_owned())),
            Some(PathBuf::from(&home).join("profiles/work"))
        );
        // A bare `~` (no slash) is a legitimate relative name; leave it alone.
        assert_eq!(
            parse_user_data_dir(Some("~".to_owned())),
            Some(PathBuf::from("~"))
        );
        assert_eq!(
            parse_user_data_dir(Some("/tmp/p".to_owned())),
            Some(PathBuf::from("/tmp/p"))
        );
        assert_eq!(
            parse_cdp_url(Some("  http://127.0.0.1:9222 ".to_owned())),
            Some("http://127.0.0.1:9222".to_owned())
        );
    }
}
