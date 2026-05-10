use clap::Parser;
use agents_connector::cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Start { session } => {
            anyhow::bail!("not yet implemented: start {}", session);
        }
        Command::Add { cli_kind, name, .. } => {
            anyhow::bail!("not yet implemented: add {} {}", cli_kind, name);
        }
        Command::List => anyhow::bail!("not yet implemented: list"),
        Command::Stop { session, .. } => anyhow::bail!("not yet implemented: stop {}", session),
        Command::Attach { session } => anyhow::bail!("not yet implemented: attach {}", session),
        Command::Tail { .. } => anyhow::bail!("not yet implemented: tail"),
        Command::Broker { socket, db } => {
            use agents_connector::broker::{server, store::Store};
            use std::sync::Arc;
            let store = Arc::new(Store::open(&db)?);
            server::serve(store, &socket).await?;
            Ok(())
        }
        Command::McpShim { .. } => anyhow::bail!("not yet implemented: mcp-shim"),
        Command::Hook { .. } => anyhow::bail!("not yet implemented: hook"),
    }
}
