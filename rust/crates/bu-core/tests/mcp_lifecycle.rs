#![cfg(all(feature = "live-chrome", unix))]

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{ChildStdin, ChildStdout, Command},
};

async fn send(stdin: &mut ChildStdin, value: Value) -> Result<()> {
    stdin.write_all(format!("{value}\n").as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}
async fn response(lines: &mut Lines<BufReader<ChildStdout>>, id: u64) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            let line = lines
                .next_line()
                .await?
                .context("MCP exited before reply")?;
            let value: Value = serde_json::from_str(&line)?;
            if value["id"] == id {
                return Ok(value);
            }
        }
    })
    .await?
}

// Kills only the child PID this fixture discovered, including after an assertion.
struct OwnedPid(String);
impl Drop for OwnedPid {
    fn drop(&mut self) {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &self.0])
            .stderr(Stdio::null())
            .status();
    }
}

#[tokio::test]
async fn eof_and_sigterm_reap_owned_browser_even_when_stopped() -> Result<()> {
    for signal in [false, true] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_browser-use-rs"))
            .arg("--mcp")
            .env("BROWSER_USE_HEADLESS", "true")
            .env("BROWSER_USE_ENV_FILE", "/nonexistent/browser-use-mcp-proof")
            .env_remove("BROWSER_USE_CDP_URL")
            .env_remove("BROWSER_USE_ALLOWED_DOMAINS")
            .env_remove("BROWSER_USE_PROHIBITED_DOMAINS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let pid = child.id().context("MCP pid")?;
        let mut stdin = child.stdin.take().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        send(&mut stdin, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"lifecycle-proof","version":"1"}}})).await?;
        assert!(response(&mut lines, 1).await?["result"].is_object());
        send(
            &mut stdin,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await?;
        send(&mut stdin, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"data:text/html,<title>Lifecycle proof</title><main>ok</main>"}}})).await?;
        let result = response(&mut lines, 2).await?;
        assert_ne!(result["result"]["isError"], true, "{result}");
        let children = Command::new("pgrep")
            .args(["-P", &pid.to_string()])
            .output()
            .await?;
        let browser_pid = String::from_utf8(children.stdout)?
            .lines()
            .next()
            .context("owned browser child")?
            .to_owned();
        let browser = OwnedPid(browser_pid);
        assert!(Command::new("kill")
            .args(["-STOP", &browser.0])
            .status()
            .await?
            .success());
        // A request blocked in CDP must not delay shutdown behind its timeout.
        send(&mut stdin, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"browser_get_state","arguments":{}}})).await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        if signal {
            assert!(Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status()
                .await?
                .success());
        }
        drop(stdin);
        let status = tokio::time::timeout(Duration::from_secs(18), child.wait()).await??;
        assert!(status.success(), "MCP shutdown failed: {status}");
        let exists = Command::new("kill")
            .args(["-0", &browser.0])
            .stderr(Stdio::null())
            .status()
            .await?;
        assert!(!exists.success(), "owned browser survived MCP exit");
    }
    Ok(())
}
