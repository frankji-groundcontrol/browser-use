use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Keep MCP noise off stdout (JSON-RPC). Hosts that set RUST_LOG still get
    // diagnostics on stderr for shutdown/orphan sweeps.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();

    if std::env::args().any(|arg| arg == "--mcp") {
        bu_mcp::run_stdio_server().await?;
    }

    Ok(())
}
