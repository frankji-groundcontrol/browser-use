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

    let page_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let page_addr = page_listener.local_addr()?;
    std::thread::spawn(move || {
        while let Ok((mut stream, _)) = page_listener.accept() {
            use std::io::Write;
            let body = "<title>Existing Tab</title><main>ok</main>";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    let page_url = format!("http://{page_addr}/");

    let mut child = std::process::Command::new(executable)
        .arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--headless=new")
        .arg("--no-sandbox")
        .arg("--no-first-run")
        .arg(&page_url)
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

    let mut tabs = Vec::new();
    for _ in 0..40 {
        tabs = session.tabs(None).await?;
        if tabs.iter().any(|tab| tab.title == "Existing Tab") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        tabs.iter().any(|tab| tab.title == "Existing Tab"),
        "attach must discover the pre-existing titled tab, got {tabs:?}"
    );

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

#[tokio::test]
#[cfg(unix)]
async fn stopped_owned_browser_is_killed_and_reaped_within_budget() -> anyhow::Result<()> {
    let session = BrowserSession::launch_headless().await?;
    let pid = session
        .browser
        .lock()
        .await
        .get_mut_child()
        .unwrap()
        .as_mut_inner()
        .id()
        .unwrap();
    let stopped = std::process::Command::new("kill")
        .args(["-STOP", &pid.to_string()])
        .status()?;
    assert!(stopped.success());
    tokio::time::timeout(std::time::Duration::from_secs(18), session.close()).await??;
    assert!(
        session.browser.lock().await.try_wait()?.is_some(),
        "owned child was not reaped"
    );
    Ok(())
}

#[tokio::test]
async fn click_element_activates_an_offscreen_one_pixel_button() -> anyhow::Result<()> {
    // Flutter Web's "Enable accessibility" control is a 1×1 node at (-1, -1).
    // A compositor mouse event at that centre misses the node; a DOM click
    // must still fire the handler. A full-viewport cover would steal a clamped
    // (0, 0) mouse click — the title must become "clicked", never "cover".
    let session = BrowserSession::launch_headless().await?;
    let page = session.new_page().await?;
    page.navigate(
        "data:text/html,<title>Offscreen</title>\
<div style='position:fixed;left:0;top:0;width:100%;height:100%' onclick='document.title=\"cover\"'></div>\
<button aria-label='Enable accessibility' \
style='position:absolute;left:-1px;top:-1px;width:1px;height:1px;padding:0;margin:0;border:0' \
onclick='document.title=\"clicked\"'>Enable accessibility</button>",
    )
    .await?;

    let elements = page.selector_map().await?;
    let target = elements
        .iter()
        .find(|element| element.text.contains("Enable accessibility"))
        .expect("the 1px offscreen button must stay in the selector map");
    page.click_element(target.index).await?;

    let title = page.state().await?.title;
    assert_eq!(
        title, "clicked",
        "offscreen 1px node must be DOM-activated, not mouse-clicked at its box centre"
    );
    Ok(())
}

/// Records, on the page title, whether the pointer sequence a click produced was
/// a real one. A synthetic `HTMLElement.click()` is untrusted and emits no
/// `pointerdown`, so this tells the two activation paths apart where "the handler
/// ran" does not.
const POINTER_LOG: &str = "onpointerdown='(window.log=window.log||[]).push(\"pd:\"+event.isTrusted)' \
onclick='(window.log=window.log||[]).push(\"click:\"+event.isTrusted);document.title=window.log.join()'";

#[tokio::test]
async fn click_element_uses_a_trusted_mouse_event_for_an_in_viewport_button() -> anyhow::Result<()>
{
    let session = BrowserSession::launch_headless().await?;
    let page = session.new_page().await?;
    page.navigate(&format!(
        "data:text/html,<title>Onscreen</title>\
<button style='position:absolute;left:20px;top:20px;width:200px;height:40px' {POINTER_LOG}>Log in</button>"
    ))
    .await?;

    let elements = page.selector_map().await?;
    let target = elements
        .iter()
        .find(|element| element.text.contains("Log in"))
        .expect("in-viewport button must be in the selector map");
    page.click_element(target.index).await?;

    assert_eq!(
        page.state().await?.title,
        "pd:true,click:true",
        "an in-viewport button needs the trusted pointer sequence, not a synthetic click()"
    );
    Ok(())
}

#[tokio::test]
async fn click_element_keeps_the_mouse_for_a_partly_scrolled_tall_element() -> anyhow::Result<()> {
    // The box starts 30px above the viewport but its centre is on screen. Testing
    // the box's top-left would send it down the synthetic path, which pointer-driven
    // widgets ignore while the tool still reports success.
    let session = BrowserSession::launch_headless().await?;
    let page = session.new_page().await?;
    page.navigate(&format!(
        "data:text/html,<title>Clipped</title>\
<button style='position:absolute;left:20px;top:-30px;width:200px;height:400px' {POINTER_LOG}>Tall menu item</button>"
    ))
    .await?;

    let elements = page.selector_map().await?;
    let target = elements
        .iter()
        .find(|element| element.text.contains("Tall menu item"))
        .expect("a partly visible button must be in the selector map");
    page.click_element(target.index).await?;

    assert_eq!(page.state().await?.title, "pd:true,click:true");
    Ok(())
}

#[tokio::test]
async fn selector_map_reports_checked_false_when_nothing_on_the_page_is_checked(
) -> anyhow::Result<()> {
    // Chromium's sparse `inputChecked` data may be absent when no box is checked;
    // that must read as "unchecked", not as "no checked state".
    let session = BrowserSession::launch_headless().await?;
    let page = session.new_page().await?;
    page.navigate(
        "data:text/html,<title>Unchecked</title>\
<input type='CHECKBOX' aria-label='Newsletter'><input type=radio aria-label='Plan'>",
    )
    .await?;

    let elements = page.selector_map().await?;
    for label in ["Newsletter", "Plan"] {
        let element = elements
            .iter()
            .find(|element| element.text.contains(label))
            .unwrap_or_else(|| panic!("{label} must be in the selector map"));
        assert_eq!(element.checked, Some(false), "{label}: {element:?}");
    }
    Ok(())
}

#[tokio::test]
async fn click_by_stale_node_id_refuses_a_button_hidden_since_the_snapshot() -> anyhow::Result<()> {
    // A node with no box (display:none) has no content quad. It used to be
    // re-checked against the live selector map and refused; the DOM-click path
    // must keep that check rather than `.click()` something the user cannot see.
    let session = BrowserSession::launch_headless().await?;
    let page = session.new_page().await?;
    page.navigate(
        "data:text/html,<title>Hidden</title>\
<button id='b' onclick='document.title=\"clicked\"'>Delete account</button>",
    )
    .await?;

    let elements = page.selector_map().await?;
    let target = elements
        .iter()
        .find(|element| element.text.contains("Delete account"))
        .expect("the button is visible when the snapshot is taken");
    let backend_node_id = target.backend_node_id_value();
    page.evaluate_json("document.getElementById('b').style.display='none'; true")
        .await?;

    assert!(
        page.click_backend_node_id(backend_node_id).await.is_err(),
        "a hidden node must not be clicked"
    );
    assert_eq!(page.state().await?.title, "Hidden");
    Ok(())
}

#[tokio::test]
async fn click_element_activates_an_offscreen_svg_node() -> anyhow::Result<()> {
    // SVG elements have no click(); the DOM path has to dispatch the event, and
    // must not report the TypeError as a bare "Uncaught".
    let session = BrowserSession::launch_headless().await?;
    let page = session.new_page().await?;
    page.navigate(
        "data:text/html,<title>Svg</title>\
<svg role='button' aria-label='Close dialog' tabindex='0' width='1' height='1' \
style='position:absolute;left:-1px;top:-1px' \
onclick='document.title=\"svg-clicked\"'><rect width='1' height='1'/></svg>",
    )
    .await?;

    let elements = page.selector_map().await?;
    let target = elements
        .iter()
        .find(|element| element.text.contains("Close dialog"))
        .expect("the offscreen svg button must be in the selector map");
    page.click_element(target.index).await?;

    assert_eq!(page.state().await?.title, "svg-clicked");
    Ok(())
}

/// Polling `state()` while a script-triggered navigation commits. chromiumoxide's
/// `evaluate` pins the execution context id it cached from the previous document;
/// its event loop lags Chrome (more so under CPU load), so the pinned id can
/// already be destroyed and Chrome answers "Cannot find context with specified
/// id". The busy threads widen that window.
#[tokio::test]
async fn state_survives_script_navigation_under_cpu_load() -> anyhow::Result<()> {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let hogs: Vec<_> = (0..std::thread::available_parallelism()?.get() * 2)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    std::hint::spin_loop();
                }
            })
        })
        .collect();

    let result = async {
        // Chrome refuses script-initiated top-level navigation to data: URLs, so
        // serve the pages over loopback; the title echoes the request path.
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let origin = format!("http://{}", listener.local_addr()?);
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for mut socket in listener.incoming().flatten() {
                let mut request = [0; 2048];
                let n = socket.read(&mut request).unwrap_or(0);
                let path = String::from_utf8_lossy(&request[..n])
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .to_string();
                let body = format!("<title>{}</title>", path.trim_start_matches('/'));
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        let session = BrowserSession::launch_headless().await?;
        let page = session.new_page().await?;
        page.navigate(&format!("{origin}/start")).await?;
        for round in 0..10 {
            let target = format!("{origin}/round-{round}");
            page.evaluate_json(&format!(
                "setTimeout(() => location.href = {}, 0); true",
                serde_json::to_string(&target)?
            ))
            .await?;
            tokio::time::timeout(std::time::Duration::from_secs(20), async {
                while page.state().await?.url != target {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                anyhow::Ok(())
            })
            .await??;
            assert_eq!(page.state().await?.title, format!("round-{round}"));
        }
        anyhow::Ok(())
    }
    .await;

    stop.store(true, Ordering::Relaxed);
    for hog in hogs {
        let _ = hog.join();
    }
    result
}

#[tokio::test]
async fn a_password_field_stays_redacted_after_a_reveal_toggle_flips_its_type() -> anyhow::Result<()>
{
    // The field has no password-like name, id, label or autocomplete, so once the
    // toggle sets type=text nothing in a snapshot marks it. Only the earlier
    // snapshot, where it was type=password, can.
    let session = BrowserSession::launch_headless().await?;
    let page = session.new_page().await?;
    page.navigate(
        "data:text/html,<title>Reveal</title>\
<input id='pw' type='password' placeholder='Secret entry'>\
<input id='plain' placeholder='Plain entry'>",
    )
    .await?;
    page.selector_map().await?;

    page.evaluate_json(
        "document.getElementById('pw').value = 'hunter2'; \
         document.getElementById('plain').value = 'visible'; \
         document.getElementById('pw').type = 'text'; true",
    )
    .await?;

    let elements = page.selector_map().await?;
    let value_of = |label: &str| {
        elements
            .iter()
            .find(|element| element.text.contains(label))
            .unwrap_or_else(|| panic!("{label} must be in the selector map: {elements:?}"))
            .value
            .clone()
    };
    assert_eq!(value_of("Secret entry"), None, "a revealed password leaked");
    assert_eq!(value_of("Plain entry").as_deref(), Some("visible"));

    // The memory belongs to a document: a new one starts with none, so ids that
    // another renderer reuses cannot hide an unrelated field.
    let target = page.target_id();
    assert!(!page.sensitive_nodes_guard()[&target].is_empty());
    page.navigate("about:blank").await?;
    assert!(page.sensitive_nodes_guard().get(&target).is_none());
    Ok(())
}
