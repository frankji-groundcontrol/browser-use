use super::*;

#[tokio::test]
async fn scoped_policy_set_get_restore_roundtrips() {
    // The retry tool's allowed_domains scoping relies on set_policy returning
    // the exact prior policy so it can be restored verbatim.
    let actor = ActorHandle::spawn();
    let base = actor.get_policy().await.unwrap();

    let scoped = UrlPolicy {
        disable_security: false,
        allowed_domains: vec!["example.com".to_owned()],
        prohibited_domains: Vec::new(),
        block_ip_addresses: false,
    };
    let previous = actor.set_policy(scoped.clone()).await.unwrap();
    assert_eq!(
        previous, base,
        "set_policy must return the exact prior policy"
    );
    assert_eq!(actor.get_policy().await.unwrap(), scoped);

    actor.set_policy(previous.clone()).await.unwrap();
    assert_eq!(
        actor.get_policy().await.unwrap(),
        previous,
        "base policy must restore verbatim"
    );
}

#[tokio::test]
async fn state_screenshot_degrades_to_none_instead_of_sinking_the_dom() {
    use std::time::Duration;

    let budget = Duration::from_millis(20);

    assert_eq!(
        screenshot_or_none(budget, async { Ok(vec![1u8, 2, 3]) }).await,
        Some(vec![1u8, 2, 3]),
        "a working capture is still returned"
    );

    assert_eq!(
        screenshot_or_none(budget, async { Err(anyhow::anyhow!("capture failed")) }).await,
        None,
        "a failed capture must degrade, not propagate"
    );

    // The DOM is already in hand by the time the screenshot runs, so a capture
    // that never comes back must not take the whole state snapshot down with it.
    assert_eq!(
        screenshot_or_none(budget, async {
            std::future::pending::<()>().await;
            unreachable!()
        })
        .await,
        None,
        "a stalled capture must be cut at the budget"
    );
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn a_capture_that_never_completes_leaves_no_stale_indices() {
    // click/type resolve an LLM-supplied index through the selector cache, so a
    // mapping that outlives the capture that produced it is how you act on the
    // wrong element. Once a capture cannot complete, nothing may be served from
    // it. Both mechanisms enforce this: get_state drops the mapping before
    // capturing and only repopulates on success, and a command cancelled on the
    // actor timeout (which runs no error branch at all) drops it too.
    let actor = ActorHandle::spawn();
    actor
        .navigate(
            "data:text/html,<title>cache</title><button>One</button><button>Two</button>"
                .to_owned(),
            false,
        )
        .await
        .expect("navigate should launch the browser");
    actor
        .set_command_timeout(std::time::Duration::from_secs(2))
        .await
        .unwrap();

    actor.get_state(false).await.expect("first capture");
    assert!(
        actor.selector_cache_len().await.unwrap() > 0,
        "a good capture should populate the index mapping"
    );

    // Wedge the renderer: every later capture now times out mid-flight.
    let _ = actor.evaluate("while (true) {}").await;

    assert_eq!(
        actor.selector_cache_len().await.unwrap(),
        0,
        "a cancelled command must leave no index behind for a later click/type"
    );

    let wedged_capture = actor.get_state(false).await;
    assert!(
        wedged_capture.is_err(),
        "capture should time out while wedged"
    );
    assert_eq!(
        actor.selector_cache_len().await.unwrap(),
        0,
        "a capture that never completed must not resurrect the previous mapping"
    );
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn wedged_command_times_out_and_actor_survives() {
    // A renderer that spins forever must not hang the actor: the command is
    // dropped on the per-command timeout and later commands still respond.
    let actor = ActorHandle::spawn();

    actor
        .navigate("data:text/html,<title>wedge</title>".to_owned(), false)
        .await
        .expect("initial navigate should launch the browser");
    actor
        .set_command_timeout(std::time::Duration::from_secs(2))
        .await
        .unwrap();

    // Runtime.evaluate on an infinite loop never returns; the actor must drop
    // it at the ~2s timeout rather than hang forever.
    let wedged = actor.evaluate("while (true) {}").await;
    assert!(
        wedged.is_err(),
        "wedged evaluate should time out, got {wedged:?}"
    );

    // A subsequent non-browser command must still respond promptly, proving
    // the actor loop was not deadlocked by the dropped command.
    let survived =
        tokio::time::timeout(std::time::Duration::from_secs(5), actor.get_policy()).await;
    assert!(
        matches!(survived, Ok(Ok(_))),
        "actor must still answer commands after a wedged one: {survived:?}"
    );
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn shutdown_cancels_wedged_work_before_reaping() -> Result<()> {
    let actor = ActorHandle::spawn();
    actor
        .navigate("data:text/html,<title>shutdown</title>".into(), false)
        .await?;
    let pending = {
        let actor = actor.clone();
        tokio::spawn(async move { actor.evaluate("while (true) {}").await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    tokio::time::timeout(std::time::Duration::from_secs(18), actor.shutdown()).await??;
    assert!(pending.await?.is_err());
    assert!(actor.get_policy().await.is_err());
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn cached_page_recovers_after_owned_browser_disconnect() -> Result<()> {
    let session = BrowserSession::launch_headless().await?;
    let page = session.primary_page().await?;
    page.navigate("data:text/html,<title>Before disconnect</title>")
        .await?;
    session.close().await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while session.is_healthy() {
            tokio::task::yield_now().await;
        }
    })
    .await?;

    let mut browser_actor = BrowserActor::new(None);
    browser_actor.session = Some(session);
    browser_actor.page = Some(page);
    let (tx, rx) = mpsc::channel(4);
    let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel();
    tokio::spawn(browser_actor.run(rx, shutdown_rx));
    let actor = ActorHandle { tx, shutdown_tx };

    let recovered =
        tokio::time::timeout(std::time::Duration::from_secs(10), actor.page_state()).await?;
    let _ = actor.shutdown().await;
    assert!(recovered.is_ok(), "cached page must recover: {recovered:?}");
    assert_ne!(recovered?.title, "Before disconnect");
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn invalid_native_input_keeps_the_existing_value() -> Result<()> {
    let actor = ActorHandle::spawn();
    actor
        .navigate(
            "data:text/html,<input id='date' type='date' value='2026-09-12' aria-label='Date'>"
                .into(),
            false,
        )
        .await?;
    let state = actor.get_state(false).await?;
    let index = state
        .elements
        .iter()
        .find(|element| element.tag == "input")
        .expect("native date input")
        .index;
    let result = actor.type_text(index, "not-a-date".into()).await;
    let value = actor
        .evaluate("document.getElementById('date').value")
        .await?;
    actor.shutdown().await?;
    assert!(
        result.is_err(),
        "invalid native date must be reported as an error"
    );
    assert_eq!(
        value, "2026-09-12",
        "invalid input must not erase form data"
    );
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn typing_rechecks_policy_after_clear_triggers_navigation() -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/allowed", listener.local_addr()?);
    let server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut request = [0; 2048];
            let _ = stream.read(&mut request).await;
            let body = r#"<input aria-label="Field" value="old" oninput="history.replaceState({}, '', '/blocked')">"#;
            let _ = stream.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()
            ).as_bytes()).await;
        }
    });
    let actor = ActorHandle::spawn();
    actor.navigate(url.clone(), false).await?;
    actor
        .set_policy(UrlPolicy {
            allowed_domains: vec![url],
            ..Default::default()
        })
        .await?;
    let state = actor.get_state(false).await?;
    let index = state
        .elements
        .iter()
        .find(|element| element.tag == "input")
        .expect("input")
        .index;
    let result = actor.type_text(index, "replacement".into()).await;
    actor.shutdown().await?;
    server.abort();
    assert!(
        result.is_err(),
        "clearing must not bypass the next URL policy check"
    );
    Ok(())
}

#[cfg(all(feature = "live-chrome", unix))]
mod recovery_policy {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct OwnedChrome {
        child: std::process::Child,
        profile: std::path::PathBuf,
    }

    impl Drop for OwnedChrome {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.profile);
        }
    }

    async fn chrome_endpoint() -> Result<(OwnedChrome, String)> {
        let executable = bu_cdp::BrowserLaunchOptions::from_env()
            .executable_path
            .or_else(|| {
                [
                    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
                    "/usr/bin/chromium",
                    "/usr/bin/google-chrome",
                    "/usr/bin/chromium-browser",
                ]
                .into_iter()
                .map(std::path::PathBuf::from)
                .find(|path| path.is_file())
            })
            .context("local Chrome executable required for reconnect policy regression")?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")?
            .local_addr()?
            .port();
        let profile = std::env::temp_dir().join(format!(
            "bu-policy-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        std::fs::create_dir(&profile)?;
        let child = std::process::Command::new(executable)
            .arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--headless=new",
                "--no-sandbox",
                "--no-first-run",
                "about:blank",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let chrome = OwnedChrome { child, profile };
        let websocket = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut stream = loop {
                if let Ok(stream) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await { break stream; }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            };
            stream.write_all(format!("GET /json/version HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
            let mut response = Vec::new();
            loop {
                let mut bytes = [0; 4096];
                let read = stream.read(&mut bytes).await?;
                anyhow::ensure!(read > 0, "DevTools closed before providing its endpoint");
                response.extend_from_slice(&bytes[..read]);
                if let Some(end) = response.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&response[end + 4..]) {
                        return Ok::<_, anyhow::Error>(value["webSocketDebuggerUrl"].as_str().context("missing DevTools endpoint")?.to_owned());
                    }
                }
            }
        }).await??;
        Ok((chrome, websocket))
    }

    async fn websocket_frame(
        stream: &mut tokio::net::tcp::OwnedReadHalf,
    ) -> std::io::Result<(Vec<u8>, Option<serde_json::Value>)> {
        let mut head = [0; 2];
        stream.read_exact(&mut head).await?;
        let mut wire = head.to_vec();
        let length = match head[1] & 127 {
            126 => {
                let mut bytes = [0; 2];
                stream.read_exact(&mut bytes).await?;
                wire.extend_from_slice(&bytes);
                u16::from_be_bytes(bytes) as u64
            }
            127 => {
                let mut bytes = [0; 8];
                stream.read_exact(&mut bytes).await?;
                wire.extend_from_slice(&bytes);
                u64::from_be_bytes(bytes)
            }
            length => length as u64,
        };
        if length > 1024 * 1024 {
            return Err(std::io::Error::other("CDP test frame exceeds limit"));
        }
        let mut mask = [0; 4];
        if head[1] & 128 != 0 {
            stream.read_exact(&mut mask).await?;
            wire.extend_from_slice(&mask);
        }
        let mut payload = vec![0; length as usize];
        stream.read_exact(&mut payload).await?;
        wire.extend_from_slice(&payload);
        if head[1] & 128 != 0 {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        let message = (head[0] & 15 == 1)
            .then(|| serde_json::from_slice(&payload).ok())
            .flatten();
        Ok((wire, message))
    }

    struct ProbeDropProxy {
        url: String,
        armed: Arc<std::sync::atomic::AtomicBool>,
        task: tokio::task::JoinHandle<()>,
    }

    impl ProbeDropProxy {
        async fn start(upstream_url: &str) -> Result<Self> {
            let (upstream, path) = upstream_url
                .strip_prefix("ws://")
                .context("local WebSocket URL")?
                .split_once('/')
                .context("DevTools path")?;
            let upstream = upstream.to_owned();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let url = format!("ws://{}/{path}", listener.local_addr()?);
            let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let probes = Arc::new(AtomicUsize::new(0));
            let trigger = armed.clone();
            let task = tokio::spawn(async move {
                while let Ok((client, _)) = listener.accept().await {
                    let upstream = upstream.clone();
                    let trigger = trigger.clone();
                    let probes = probes.clone();
                    tokio::spawn(async move {
                        let result = async {
                            let browser = tokio::net::TcpStream::connect(upstream).await?;
                            let (mut client_read, mut client_write) = client.into_split();
                            let (mut browser_read, mut browser_write) = browser.into_split();
                            let mut headers = Vec::new();
                            while !headers.ends_with(b"\r\n\r\n") { headers.push(client_read.read_u8().await?); }
                            browser_write.write_all(&headers).await?;
                            let outgoing = async {
                                loop {
                                    let (frame, message) = websocket_frame(&mut client_read).await?;
                                    if trigger.load(Ordering::SeqCst) && message.as_ref().is_some_and(|value| value["method"] == "Browser.getVersion") && probes.fetch_add(1, Ordering::SeqCst) == 1 {
                                        trigger.store(false, Ordering::SeqCst);
                                        return Ok::<(), std::io::Error>(());
                                    }
                                    browser_write.write_all(&frame).await?;
                                }
                            };
                            tokio::select! {
                                result = outgoing => result,
                                result = tokio::io::copy(&mut browser_read, &mut client_write) => result.map(|_| ()),
                            }
                        }.await;
                        let _ = result;
                    });
                }
            });
            Ok(Self { url, armed, task })
        }
    }

    impl Drop for ProbeDropProxy {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    struct CdpEnvironment(Option<std::ffi::OsString>);
    impl Drop for CdpEnvironment {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("BROWSER_USE_CDP_URL", value),
                None => std::env::remove_var("BROWSER_USE_CDP_URL"),
            }
        }
    }

    async fn guarded_observation(snapshot: bool) -> Result<()> {
        let (_chrome, websocket) = chrome_endpoint().await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let base = format!("http://{}", listener.local_addr()?);
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut request = [0; 2048];
                    let read = stream.read(&mut request).await.unwrap_or(0);
                    let body = if request[..read].starts_with(b"GET /allowed ") {
                        "<title>Allowed</title><main>ALLOWED_CONTENT</main>"
                    } else {
                        "<title>Denied</title><main>DENIED_CONTENT</main>"
                    };
                    let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
                });
            }
        });
        let owner = BrowserSession::attach(&websocket).await?;
        let first = owner.primary_page().await?;
        let second = owner.new_page().await?;
        let (denied, allowed) = if first.target_id() < second.target_id() {
            (first, second)
        } else {
            (second, first)
        };
        denied.navigate(&format!("{base}/denied")).await?;
        allowed.navigate(&format!("{base}/allowed")).await?;
        let proxy = ProbeDropProxy::start(&websocket).await?;
        let session = BrowserSession::attach(&proxy.url).await?;
        let page = session.switch_tab(&allowed.target_id()).await?;
        let mut actor = BrowserActor::new(None);
        actor.policy = UrlPolicy {
            allowed_domains: vec![format!("{base}/allowed")],
            ..Default::default()
        };
        actor.session = Some(session);
        actor.page = Some(page);
        let _environment = CdpEnvironment(std::env::var_os("BROWSER_USE_CDP_URL"));
        std::env::set_var("BROWSER_USE_CDP_URL", &proxy.url);
        proxy.armed.store(true, Ordering::SeqCst);
        let observed = if snapshot {
            tokio::time::timeout(std::time::Duration::from_secs(15), actor.get_state(false))
                .await?
                .map(|state| state.page.title)
        } else {
            tokio::time::timeout(std::time::Duration::from_secs(15), actor.get_html(None)).await?
        };
        actor.close_all().await?;
        server.abort();
        assert!(
            observed.as_ref().is_ok_and(|content| if snapshot {
                content == "Allowed"
            } else {
                content.contains("ALLOWED_CONTENT") && !content.contains("DENIED_CONTENT")
            }),
            "content must come from the validated allowed tab: {observed:?}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn content_uses_the_page_that_passed_policy_before_reconnect() -> Result<()> {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            guarded_observation(false),
        )
        .await?
    }

    #[tokio::test]
    async fn snapshot_uses_the_page_and_session_that_passed_policy() -> Result<()> {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            guarded_observation(true),
        )
        .await?
    }
}
