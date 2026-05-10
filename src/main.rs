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
            agents_connector::subcommands::start::run(&session, None)
        }
        Command::Add { cli_kind, name, .. } => {
            anyhow::bail!("not yet implemented: add {} {}", cli_kind, name);
        }
        Command::List => agents_connector::subcommands::list::run(),
        Command::Stop { session, kill_tmux } => {
            agents_connector::subcommands::stop::run(&session, kill_tmux).await
        }
        Command::Attach { session } => agents_connector::subcommands::attach::run(&session),
        Command::Tail { session } => agents_connector::subcommands::tail::run(session).await,
        Command::Broker { socket, db } => {
            use agents_connector::broker::{server, store::Store};
            use std::sync::Arc;
            let store = Arc::new(Store::open(&db)?);
            server::serve(store, &socket).await?;
            Ok(())
        }
        Command::McpShim { socket, agent_token } => {
            agents_connector::shim::run(socket, agent_token).await
        }
        Command::Hook { .. } => anyhow::bail!("not yet implemented: hook"),
    }
}
