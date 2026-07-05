use anyhow::Result;

fn main() -> Result<()> {
    if std::env::args().any(|arg| arg == "--mcp") {
        println!("TODO: Rust MCP server is not implemented yet");
    }

    Ok(())
}
