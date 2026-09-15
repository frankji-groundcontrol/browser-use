//! `browser-use-rs` entry point.
//!
//! Thin dispatch only: argument parsing lives in [`cli`], the MCP server in
//! `bu-mcp`, and the agent loop in `bu-agent`.

mod cli;
mod run;

use anyhow::Result;

use cli::{Command, EXIT_USAGE, USAGE};

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

    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse(&args) {
        Ok(command) => command,
        Err(error) => {
            // A usage error must be loud and non-zero: a mistyped flag used to
            // start a process that did nothing and reported success.
            eprintln!("browser-use-rs: {error}\n\n{USAGE}");
            std::process::exit(EXIT_USAGE);
        }
    };

    match command {
        Command::Mcp => bu_mcp::run_stdio_server().await?,
        Command::Help => print!("{USAGE}"),
        Command::Version => println!("browser-use-rs {}", env!("CARGO_PKG_VERSION")),
        Command::Run(options) => run::run_task(options).await?,
    }

    Ok(())
}
