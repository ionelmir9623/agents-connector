use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "agents-connector", version, about = "Multi-agent CLI communication substrate")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start a new session (creates broker + tmux).
    Start { session: String },
    /// Add an agent to the current session.
    Add {
        cli_kind: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        workdir: Option<std::path::PathBuf>,
    },
    /// List all sessions.
    List,
    /// Remove an agent from the current session.
    Remove {
        #[arg(long)]
        name: String,
        #[arg(long)]
        session: Option<String>,
    },
    /// Restart an agent in place (same identity, fresh model context).
    Restart {
        #[arg(long)]
        name: String,
        #[arg(long)]
        session: Option<String>,
    },
    /// Stop a running session.
    Stop {
        session: String,
        #[arg(long)]
        kill_tmux: bool,
    },
    /// Attach to a running session's tmux.
    Attach { session: String },
    /// Tail the chat transcript of a session.
    Tail {
        session: Option<String>,
    },
    /// Internal: run the broker daemon. Users should not invoke directly.
    #[command(hide = true)]
    Broker {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        db: std::path::PathBuf,
    },
    /// Internal: run the MCP shim. Users should not invoke directly.
    #[command(hide = true)]
    McpShim {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        agent_token: String,
    },
    /// Internal: invoked by adapter hooks (e.g., Claude Code Stop hook).
    #[command(hide = true)]
    Hook {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        agent_token: String,
        #[arg(long)]
        event: String,
    },
}
