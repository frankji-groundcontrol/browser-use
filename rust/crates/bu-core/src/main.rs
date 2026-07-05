use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--mcp") {
        bu_mcp::run_stdio_server().await?;
    }

    Ok(())
}
