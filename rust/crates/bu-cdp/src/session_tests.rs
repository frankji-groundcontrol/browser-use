use super::BrowserSession;

struct LocalHttpFixture {
    url: String,
    task: tokio::task::JoinHandle<()>,
}

impl LocalHttpFixture {
    async fn start() -> anyhow::Result<Self> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = [0u8; 2048];
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(1),
                    stream.read(&mut request),
                )
                .await;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                    .await;
            }
        });
        Ok(Self { url, task })
    }
}

impl Drop for LocalHttpFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn disable_security_environment_allows_cross_origin_requests() -> anyhow::Result<()> {
    let origin = LocalHttpFixture::start().await?;
    let cross_origin = LocalHttpFixture::start().await?;
    let original = std::env::var_os("BROWSER_USE_DISABLE_SECURITY");
    std::env::set_var("BROWSER_USE_DISABLE_SECURITY", "true");
    let mut options = crate::BrowserLaunchOptions::from_env();
    match original {
        Some(value) => std::env::set_var("BROWSER_USE_DISABLE_SECURITY", value),
        None => std::env::remove_var("BROWSER_USE_DISABLE_SECURITY"),
    }
    options.headless = true;
    options.user_data_dir = None;
    options.cdp_url = None;
    let session = BrowserSession::launch_with_options(options).await?;
    let page = session.primary_page().await?;
    page.navigate(&origin.url).await?;
    let allowed = page
        .evaluate_json(&format!(
            "fetch({}).then(r => r.text()).then(t => t === 'ok').catch(() => false)",
            serde_json::to_string(&cross_origin.url)?
        ))
        .await?;
    let command_line = session
        .browser
        .lock()
        .await
        .execute(
            chromiumoxide::cdp::browser_protocol::browser::GetBrowserCommandLineParams::default(),
        )
        .await?;
    session.close().await?;
    assert_eq!(
        allowed, true,
        "explicit opt-in must disable Chromium's CORS check"
    );
    assert_eq!(
        command_line
            .arguments
            .iter()
            .rev()
            .find(|arg| arg.starts_with("--disable-features=")),
        Some(&"--disable-features=TranslateUI,IsolateOrigins,site-per-process".to_owned()),
        "security opt-in must preserve the dependency's default disabled feature",
    );
    Ok(())
}

#[tokio::test]
async fn security_opt_in_never_changes_an_attached_browser() -> anyhow::Result<()> {
    let origin = LocalHttpFixture::start().await?;
    let cross_origin = LocalHttpFixture::start().await?;
    let owner = BrowserSession::launch_headless().await?;
    let page = owner.primary_page().await?;
    page.navigate(&origin.url).await?;
    let expression = format!(
        "fetch({}).then(() => true).catch(() => false)",
        serde_json::to_string(&cross_origin.url)?
    );
    assert_eq!(page.evaluate_json(&expression).await?, false);

    let websocket_url = owner.browser.lock().await.websocket_address().clone();
    let attached = BrowserSession::launch_with_options(crate::BrowserLaunchOptions {
        disable_security: true,
        cdp_url: Some(websocket_url),
        ..Default::default()
    })
    .await?;
    let attached_page = attached.primary_page().await?;
    let still_restricted = attached_page.evaluate_json(&expression).await?;
    attached.close().await?;
    owner.close().await?;
    assert_eq!(
        still_restricted, false,
        "attach must retain the owner's CORS policy"
    );
    Ok(())
}

struct CdpProxy {
    url: String,
    disconnect: Option<tokio::sync::oneshot::Sender<bool>>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl CdpProxy {
    async fn start(upstream_url: &str) -> anyhow::Result<Self> {
        use tokio::io::AsyncWriteExt;

        let mut endpoint = url::Url::parse(upstream_url)?;
        let upstream = format!(
            "{}:{}",
            endpoint.host_str().unwrap(),
            endpoint.port().unwrap()
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        endpoint
            .set_port(Some(listener.local_addr()?.port()))
            .unwrap();
        let (disconnect, closed) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let (mut client, _) = listener.accept().await?;
            let mut browser = tokio::net::TcpStream::connect(upstream).await?;
            tokio::select! {
                result = tokio::io::copy_bidirectional(&mut client, &mut browser) => { result?; }
                clean_close = closed => {
                    if clean_close == Ok(true) {
                        client.write_all(&[0x88, 0]).await?;
                    }
                    client.shutdown().await?;
                }
            }
            Ok(())
        });
        Ok(Self {
            url: endpoint.to_string(),
            disconnect: Some(disconnect),
            task,
        })
    }

    fn disconnect(&mut self, clean: bool) {
        self.disconnect.take().unwrap().send(clean).unwrap();
    }
}

impl Drop for CdpProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn connection_check_detects_disconnect(clean_close: bool) -> anyhow::Result<()> {
    let owner = BrowserSession::launch_headless().await?;
    let websocket_url = owner.browser.lock().await.websocket_address().clone();
    let mut proxy = CdpProxy::start(&websocket_url).await?;
    let attached = BrowserSession::attach(&proxy.url).await?;
    assert!(attached.is_healthy());
    proxy.disconnect(clean_close);
    let connected = attached.check_connection().await;
    drop(attached);
    owner.close().await?;
    assert!(
        !connected,
        "next connection check must detect CDP disconnect (clean close: {clean_close})"
    );
    Ok(())
}

#[tokio::test]
async fn websocket_transport_failure_invalidates_session_health() -> anyhow::Result<()> {
    connection_check_detects_disconnect(false).await
}

#[tokio::test]
async fn next_connection_check_detects_a_clean_websocket_close() -> anyhow::Result<()> {
    connection_check_detects_disconnect(true).await
}
