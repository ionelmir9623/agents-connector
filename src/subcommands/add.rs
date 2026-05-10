use crate::adapters::{claude, CliKind};
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::{paths, tmux};
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use tokio::net::UnixStream;

pub async fn run(
    cli_kind: String,
    name: String,
    session: Option<String>,
    workdir: Option<PathBuf>,
) -> Result<()> {
    let kind = CliKind::parse(&cli_kind)?;
    let session = session.or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("broker not running for `{}`. Use `agents-connector start {}` first.", session, session);
    }

    // 1. Ask broker to register the agent.
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    let workdir_str = workdir.as_ref().map(|p| p.to_string_lossy().to_string());
    let req = Request::RegisterAgent { name: name.clone(), cli_kind: kind.as_str().to_string(), workdir: workdir_str.clone() };
    write_frame_async(&mut s, &serde_json::to_vec(&req)?).await?;
    let frame = read_frame_async(&mut s).await?;
    let token = match serde_json::from_slice::<Response>(&frame)? {
        Response::RegisterAck { agent_token } => agent_token,
        Response::Error { message } => anyhow::bail!("register failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };
    drop(s);

    // 2. Generate per-CLI config.
    let agent_dir = paths::session_agent_dir(&session, &name)?;
    let exe = std::env::current_exe()?;
    let generated = match kind {
        CliKind::Claude => claude::generate(&agent_dir, &exe, &socket, &token)?,
    };

    // 3. Build the launch command. For Claude Code:
    //    claude --mcp-config <mcp.json> --settings <settings.json>
    //    (verify flag names match current Claude Code; older versions use --mcp-config-file etc.)
    let launch_cmd = match kind {
        CliKind::Claude => format!(
            "claude --mcp-config {} --settings {}",
            shell_quote(&generated.mcp_config_path.to_string_lossy()),
            shell_quote(&generated.settings_path.to_string_lossy())
        ),
    };

    // 4. tmux new-window inside the session.
    let cd_prefix = workdir_str.as_ref().map(|d| format!("cd {} && ", shell_quote(d))).unwrap_or_default();
    let full_cmd = format!("{}{}", cd_prefix, launch_cmd);
    tmux::new_window(&session, &name, &[], &full_cmd)?;

    println!("agent `{}` ({}) added to session `{}`.", name, kind.as_str(), session);
    println!("MCP config: {}", generated.mcp_config_path.display());
    println!("Settings:   {}", generated.settings_path.display());

    Ok(())
}

fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./=:".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}
