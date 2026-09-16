//! Resolve DevTools HTTP discovery endpoints without logging capability URLs.

use anyhow::{anyhow, Context, Result};
use url::Url;

pub(crate) async fn devtools_websocket_url(endpoint: &str) -> Result<String> {
    let mut endpoint = Url::parse(endpoint).context("invalid DevTools endpoint URL")?;
    if matches!(endpoint.scheme(), "ws" | "wss") {
        return Ok(endpoint.to_string());
    }
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(anyhow!("DevTools endpoint must use HTTP(S) or WS(S)"));
    }
    let path = endpoint.path().trim_end_matches('/');
    if !path.ends_with("/json/version") {
        endpoint.set_path(&format!("{path}/json/version"));
    }
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build DevTools HTTP client")?;
    let response = http
        .get(endpoint)
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .context("failed to query DevTools endpoint")?
        .error_for_status()
        .map_err(reqwest::Error::without_url)
        .context("DevTools endpoint rejected discovery request")?;
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(reqwest::Error::without_url)
        .context("invalid DevTools version response")?;
    body.get("webSocketDebuggerUrl")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("DevTools version response has no webSocketDebuggerUrl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn websocket_scheme_is_case_insensitive() {
        assert_eq!(
            devtools_websocket_url("WS://localhost:9222/devtools/browser/test")
                .await
                .unwrap(),
            "ws://localhost:9222/devtools/browser/test"
        );
    }

    #[tokio::test]
    async fn discovery_suffix_is_not_duplicated_and_query_is_preserved() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!(
            "http://{}/json/version?token=test",
            listener.local_addr().unwrap()
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let n = stream.read(&mut request).unwrap();
            assert!(
                String::from_utf8_lossy(&request[..n]).starts_with("GET /json/version?token=test ")
            );
            let body = r#"{"webSocketDebuggerUrl":"ws://localhost:9222/devtools/browser/test"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        assert!(devtools_websocket_url(&endpoint)
            .await
            .unwrap()
            .starts_with("ws://"));
        server.join().unwrap();
    }
}
