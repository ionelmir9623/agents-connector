use crate::adapters::CliKind;
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::subcommands::launch::{self, Spec};
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

    // 1. Ask broker to register the agent (with workdir).
    let workdir_str = workdir.as_ref().map(|p| p.to_string_lossy().to_string());
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    let req = Request::RegisterAgent {
        name: name.clone(),
        cli_kind: kind.as_str().to_string(),
        workdir: workdir_str.clone(),
    };
    write_frame_async(&mut s, &serde_json::to_vec(&req)?).await?;
    let frame = read_frame_async(&mut s).await?;
    let token = match serde_json::from_slice::<Response>(&frame)? {
        Response::RegisterAck { agent_token } => agent_token,
        Response::Error { message } => anyhow::bail!("register failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };
    drop(s);

    // 2. Use the shared launch helper.
    let spec = Spec {
        session: session.clone(),
        name: name.clone(),
        kind,
        token,
        workdir: workdir.clone(),
    };
    launch::launch_in_tmux(&spec, &socket)?;

    println!("agent `{}` ({}) added to session `{}`.", name, kind.as_str(), session);
    Ok(())
}
