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

    let screenshot = page
        .screenshot_image(false, crate::ScreenshotFormat::Jpeg)
        .await?;
    assert!(screenshot.starts_with(b"\xff\xd8\xff"));

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

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn a_persistent_profile_keeps_cookies_across_sessions() -> anyhow::Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    // The point of a persistent profile: log in once, stay logged in. A login is
    // a cookie, so this sets one in session 1 and asserts the server sees it come
    // back in session 2 — a fresh browser process using the same profile dir.
    // Needs a real http origin: cookies are not stored for data: URLs.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind cookie server");
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen_writer = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut chunk = [0u8; 2048];
            let read = stream.read(&mut chunk).unwrap_or(0);
            if read == 0 {
                continue;
            }
            let request = String::from_utf8_lossy(&chunk[..read]).to_string();
            let cookie = request
                .lines()
                .find_map(|line| line.strip_prefix("Cookie: "))
                .unwrap_or("")
                .trim()
                .to_owned();
            seen_writer.lock().unwrap().push(cookie);
            let body = "<title>Cookie</title>ok";
            // Max-Age makes it a persistent cookie; a session cookie would never
            // reach disk and the test would prove nothing.
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\nset-cookie: bu_login=yes; Max-Age=3600; Path=/\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });

    let profile = std::env::temp_dir().join(format!("bu-profile-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&profile);
    let options = |dir: &std::path::Path| crate::BrowserLaunchOptions {
        headless: true,
        executable_path: None,
        user_data_dir: Some(dir.to_path_buf()),
        cdp_url: None,
    };
    let url = format!("http://127.0.0.1:{port}/");

    let first = crate::BrowserSession::launch_with_options(options(&profile)).await?;
    first.new_page().await?.navigate(&url).await?;
    // Graceful close so Chromium flushes its cookie store to the profile.
    first.close().await?;
    drop(first);

    assert!(
        profile.exists(),
        "a caller-supplied profile must survive the session that used it"
    );

    let second = crate::BrowserSession::launch_with_options(options(&profile)).await?;
    second.new_page().await?.navigate(&url).await?;
    second.close().await?;
    drop(second);

    let requests = seen.lock().unwrap().clone();
    assert!(
        requests.len() >= 2,
        "expected a request per session, got {requests:?}"
    );
    assert!(
        requests[0].is_empty(),
        "first visit should carry no cookie, got {:?}",
        requests[0]
    );
    // Assert on "some later request carried it" rather than the last one: the
    // browser also makes incidental requests (favicon) that legitimately arrive
    // without the cookie, and their ordering is not ours to control.
    assert!(
        requests[1..].iter().any(|c| c.contains("bu_login=yes")),
        "the second session must reuse the stored cookie, got {requests:?}"
    );

    std::fs::remove_dir_all(&profile).ok();
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn a_throwaway_profile_is_deleted_and_stale_ones_are_swept() -> anyhow::Result<()> {
    // The privacy guarantee of the default session: no login outlives it,
    // because the whole profile goes. Two halves, because a process that is
    // killed never runs Drop at all:
    //   1. a closed session removes its own profile
    //   2. a profile abandoned by a dead session is swept by the next launch
    let session = crate::BrowserSession::launch_headless().await?;
    let dir = session
        .scratch_user_data_dir
        .as_ref()
        .map(|(dir, _lock)| dir.clone())
        .expect("a default launch owns a throwaway profile");
    assert!(dir.exists(), "profile should exist while the session lives");

    // close() waits for Chromium to exit; without that the removal races the
    // still-running process rewriting its profile.
    session.close().await?;
    drop(session);
    assert!(
        !dir.exists(),
        "a closed session must remove its own profile"
    );

    // Simulate a session killed mid-flight: a profile with an unheld lock.
    let stale = std::env::temp_dir().join(format!(
        "browser-use-rs-chromium-{}-stale",
        std::process::id()
    ));
    std::fs::create_dir_all(stale.join("Default"))?;
    std::fs::write(stale.join(".bu-owner.lock"), b"")?;
    std::fs::write(stale.join("Default").join("Cookies"), b"leftover")?;

    let next = crate::BrowserSession::launch_headless().await?;
    assert!(
        !stale.exists(),
        "a profile left by a dead session must be swept at the next launch"
    );
    next.close().await?;
    Ok(())
}

#[tokio::test]
#[cfg(feature = "live-chrome")]
async fn attaching_uses_an_existing_browser_and_never_closes_it() -> anyhow::Result<()> {
    use chromiumoxide::detection::{default_executable, DetectionOptions};

    // Stands in for "my real Chrome, already logged in": a browser this code did
    // not launch. Attaching must drive it, and close() must leave it running --
    // killing the user's browser would take their session down with it.
    let executable = default_executable(DetectionOptions::default())
        .map_err(|error| anyhow::anyhow!("no Chromium available: {error}"))?;
    let port = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0")?;
        probe.local_addr()?.port()
    };
    let profile = std::env::temp_dir().join(format!("bu-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&profile);

    let mut child = std::process::Command::new(executable)
        .arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--headless=new")
        .arg("--no-sandbox")
        .arg("--no-first-run")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    // Wait for the DevTools endpoint to come up. A plain TCP connect is the
    // readiness signal, so this needs no HTTP client dependency.
    let address = format!("127.0.0.1:{port}");
    let mut ready = false;
    for _ in 0..60 {
        if std::net::TcpStream::connect(&address).is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(ready, "external Chromium never exposed its DevTools port");

    let session = crate::BrowserSession::launch_with_options(crate::BrowserLaunchOptions {
        headless: true,
        executable_path: None,
        user_data_dir: None,
        cdp_url: Some(format!("http://127.0.0.1:{port}")),
    })
    .await?;

    let page = session.new_page().await?;
    page.navigate("data:text/html,<title>Attached</title>")
        .await?;
    assert_eq!(page.state().await?.title, "Attached");

    // close() is a no-op for a browser we did not launch.
    session.close().await?;
    drop(session);

    assert!(
        child.try_wait()?.is_none(),
        "attaching must not kill the user's browser"
    );
    assert!(
        std::net::TcpStream::connect(&address).is_ok(),
        "the attached browser should still be serving DevTools"
    );

    let _ = child.kill();
    let _ = child.wait();
    std::fs::remove_dir_all(&profile).ok();
    Ok(())
}
