//! Running the autonomous agent from the terminal.
//!
//! Wiring only — the loop itself is `bu_agent::run_task`, the same call the
//! `retry_with_browser_use_agent` MCP tool makes, so CLI and MCP cannot drift
//! in behaviour.

use anyhow::{Context, Result};
use bu_actor::ActorHandle;
use bu_llm::LlmProvider;

use crate::cli::RunOptions;

pub async fn run_task(options: RunOptions) -> Result<()> {
    // Set before the actor spawns: headless-ness is read from the environment
    // when Chromium launches, so reusing that path avoids a second mechanism.
    if options.headful {
        std::env::set_var("BROWSER_USE_HEADLESS", "false");
    }

    let config = bu_llm::LlmConfig::from_env_with_model_override(options.model)
        .context("could not resolve LLM configuration")?;
    let model = config.model.clone();
    let provider: LlmProvider = LlmProvider::from_config(config).await?;

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

    // Prioritized shutdown cancels pending work and reaps owned Chromium before
    // the runtime exits, including on unsuccessful agent reports.
    actor
        .shutdown()
        .await
        .context("failed to shut down browser")?;

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
        anyhow::bail!("agent did not complete successfully");
    }
    Ok(())
}
