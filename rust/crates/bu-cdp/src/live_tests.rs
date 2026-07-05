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

    let screenshot = page.screenshot_png(false).await?;
    assert!(screenshot.starts_with(b"\x89PNG\r\n\x1a\n"));

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
