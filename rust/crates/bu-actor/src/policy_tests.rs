use super::*;
use bu_cdp::BrowserLaunchOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn forbidden_first_page_and_mutations_fail_closed() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/", listener.local_addr()?);
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = [0; 4096];
                let _ = socket.read(&mut request).await;
                let body = "<title>private</title><button>secret</button><input><select><option>a</option></select>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let session = BrowserSession::launch_with_options(BrowserLaunchOptions::default()).await?;
    let page = session.primary_page().await?;
    page.navigate(&url).await?;
    let mut actor = BrowserActor::new(None);
    actor.session = Some(session);
    actor.policy = UrlPolicy {
        allowed_domains: vec!["example.com".into()],
        ..Default::default()
    };
    // This is the attach path before any observation: session exists, page=None.
    assert!(
        actor.get_state(false).await.is_err(),
        "first observation leaked forbidden page"
    );
    for action in [
        "coordinates",
        "click",
        "type",
        "select",
        "scroll",
        "viewport",
        "html",
    ] {
        page.navigate(&url).await?;
        actor.page = Some(page.clone());
        let result = match action {
            "coordinates" | "scroll" => {
                let (reply, rx) = oneshot::channel();
                let command = if action == "coordinates" {
                    Command::ClickCoordinates {
                        x: 10.0,
                        y: 10.0,
                        reply,
                    }
                } else {
                    Command::Scroll {
                        direction: "down".into(),
                        reply,
                    }
                };
                actor.dispatch(command).await;
                rx.await?
            }
            "click" => actor.click(1, false).await.map(|_| ()),
            "type" => actor.type_text(1, "private").await,
            "select" => actor
                .select_option(1, Some("a".into()), None, None)
                .await
                .map(|_| ()),
            "viewport" => actor.set_viewport(400, 300, false).await,
            _ => actor.get_html(None).await.map(|_| ()),
        };
        assert!(result.is_err(), "{action} acted on a forbidden page");
        assert_eq!(page.state().await?.url, "about:blank");
    }
    // A page may script-navigate after a successful observation and before an
    // indexed action. The previously valid selector must not permit that action.
    page.navigate("data:text/html,<button>allowed before redirect</button>")
        .await?;
    actor.page = Some(page.clone());
    actor.get_state(false).await?;
    page.evaluate_json(&format!(
        "setTimeout(() => location.href = {}, 0); true",
        serde_json::to_string(&url)?
    ))
    .await?;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while page.state().await?.url != url {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await??;
    assert!(actor.click(1, false).await.is_err());
    assert_eq!(page.state().await?.url, "about:blank");
    // A detached target cannot be verified: policy must propagate the error.
    actor
        .session
        .as_ref()
        .unwrap()
        .close_tab(&page.target_id())
        .await?;
    assert!(actor.get_html(None).await.is_err());
    actor.close_all().await?;
    server.abort();
    Ok(())
}
