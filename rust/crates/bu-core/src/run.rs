//! Running the autonomous agent from the terminal.
//!
//! Wiring only — the loop itself is `bu_agent::run_task`, the same call the
//! `retry_with_browser_use_agent` MCP tool makes, so CLI and MCP cannot drift
//! in behaviour.

use anyhow::{Context, Result};
use bu_actor::ActorHandle;
use bu_llm::{LlmClient, LlmProvider};

use crate::cli::RunOptions;

/// Best-effort browser shutdown budget. Short on purpose: the answer is already
/// computed by this point, so the caller should not wait on cleanup.
const CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub async fn run_task(options: RunOptions) -> Result<()> {
    // Set before the actor spawns: headless-ness is read from the environment
    // when Chromium launches, so reusing that path avoids a second mechanism.
    if options.headful {
        std::env::set_var("BROWSER_USE_HEADLESS", "false");
    }

    let config = bu_llm::LlmConfig::from_env_with_model_override(options.model)
        .context("could not resolve LLM configuration")?;
    let model = config.model.clone();
    let provider: LlmProvider = LlmClient::new(config)?.into();

    eprintln!("· model {model}, up to {} steps", options.max_steps);

    let actor = ActorHandle::spawn();
    let report = bu_agent::run_task(
        options.task,
        options.max_steps,
        actor.clone(),
        &provider,
        options.use_vision,
    )
    .await;

    // Bounded, because a browser that already died makes this block for the
    // full command timeout — 90 seconds of silence *after* the answer is ready.
    // The answer is what the caller wants; cleanup is best-effort, and the
    // actor's orphan sweep reclaims anything left behind.
    if tokio::time::timeout(CLOSE_TIMEOUT, actor.close_all())
        .await
        .is_err()
    {
        // Not an error worth the caller's attention; the sweep handles it.
    }

    // The report goes to stdout so it can be piped; progress notes went to
    // stderr above for the same reason.
    if !report.final_result.trim().is_empty() {
        println!("{}", report.final_result);
    }
    eprintln!(
        "· {} step{}, success={}",
        report.steps,
        if report.steps == 1 { "" } else { "s" },
        report.success
    );
    for error in &report.errors {
        eprintln!("· error: {error}");
    }

    // Exit non-zero when the agent did not succeed, so the CLI composes with
    // shell control flow instead of always looking like it worked.
    if !report.success {
        std::process::exit(1);
    }
    Ok(())
}
