use crate::adapters::CliKind;
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::subcommands::launch::{self, Spec};
use crate::{paths, tmux};
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use tokio::net::UnixStream;

pub async fn run(name: &str, session: Option<String>) -> Result<()> {
    let session = session.or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("broker not running for `{}`.", session);
    }

    // 1. Ask broker for full agent details.
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    write_frame_async(&mut s, &serde_json::to_vec(&Request::GetAgent { name: name.to_string() })?).await?;
    let frame = read_frame_async(&mut s).await?;
    let (cli_kind_str, token, workdir) = match serde_json::from_slice::<Response>(&frame)? {
        Response::AgentDetails { cli_kind, token, workdir, .. } => (cli_kind, token, workdir),
        Response::Error { message } => anyhow::bail!("get_agent failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };
    drop(s);

    // 1b. Reset agent state to idle before relaunch — clears any wedged busy state.
    let mut s = UnixStream::connect(&socket).await.context("connecting to broker for state reset")?;
    write_frame_async(&mut s, &serde_json::to_vec(&Request::SetAgentState {
        agent_token: token.clone(),
        state: "idle".into(),
    })?).await?;
    let _ = read_frame_async(&mut s).await?;
    drop(s);

    let kind = CliKind::parse(&cli_kind_str)?;

    // 2. Kill the existing tmux window (no-op if already dead).
    launch::kill_agent_window(&session, name)?;

    // 3. Relaunch via the shared helper.
    let spec = Spec {
        session: session.clone(),
        name: name.to_string(),
        kind,
        token,
        workdir: workdir.map(PathBuf::from),
    };
    launch::launch_in_tmux(&spec, &socket)?;

    println!("agent `{}` restarted in session `{}` (chat history preserved).", name, session);
    Ok(())
}
