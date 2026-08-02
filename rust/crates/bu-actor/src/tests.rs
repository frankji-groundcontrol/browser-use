use super::*;

#[tokio::test]
async fn scoped_policy_set_get_restore_roundtrips() {
    // The retry tool's allowed_domains scoping relies on set_policy returning
    // the exact prior policy so it can be restored verbatim.
    let actor = ActorHandle::spawn();
    let base = actor.get_policy().await.unwrap();

    let scoped = UrlPolicy {
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
async fn wedged_command_times_out_and_actor_survives() {
    // A renderer that spins forever must not hang the actor: the command is
    // dropped on the per-command timeout and later commands still respond.
    let actor = ActorHandle::spawn_with_command_timeout(std::time::Duration::from_secs(2));

    actor
        .navigate("data:text/html,<title>wedge</title>".to_owned(), false)
        .await
        .expect("initial navigate should launch the browser");

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
