use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::subcommands::launch;
use crate::{paths, tmux};
use anyhow::{anyhow, Context, Result};
use tokio::net::UnixStream;

pub async fn run(name: &str, session: Option<String>) -> Result<()> {
    let session = session.or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("broker not running for `{}`.", session);
    }

    // Send RemoveAgent IPC.
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RemoveAgent { name: name.to_string() })?).await?;
    let frame = read_frame_async(&mut s).await?;
    match serde_json::from_slice::<Response>(&frame)? {
        Response::RemoveAck { .. } => {}
        Response::Error { message } => anyhow::bail!("remove failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    }
    drop(s);

    // Kill the agent's tmux window (idempotent — fine if already gone).
    launch::kill_agent_window(&session, name)?;

    println!("agent `{}` removed from session `{}`.", name, session);
    Ok(())
}
