use crate::adapters::CliKind;
use crate::ipc::{read_frame_async, write_frame_async, AgentDto, Request, Response};
use crate::subcommands::launch::{self, Spec};
use crate::{paths, tmux};
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use tokio::net::UnixStream;

pub async fn run(
    cli_kind: String,
    name: String,
    session: Option<String>,
    workdir: Option<PathBuf>,
    extra_args: Vec<String>,
) -> Result<()> {
    let kind = CliKind::parse(&cli_kind)?;
    let session = session.or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("broker not running for `{}`. Use `agents-connector start {}` first.", session, session);
    }

    // 1. Ask broker to register the agent (with workdir and extra_args).
    let workdir_str = workdir.as_ref().map(|p| p.to_string_lossy().to_string());
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker")?;
    let req = Request::RegisterAgent {
        name: name.clone(),
        cli_kind: kind.as_str().to_string(),
        workdir: workdir_str.clone(),
        extra_args: extra_args.clone(),
    };
    write_frame_async(&mut s, &serde_json::to_vec(&req)?).await?;
    let frame = read_frame_async(&mut s).await?;
    let token = match serde_json::from_slice::<Response>(&frame)? {
        Response::RegisterAck { agent_token } => agent_token,
        Response::Error { message } => anyhow::bail!("register failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };
    drop(s);

    // 2. Fetch the current agent roster (the new agent is now in this list).
    let mut s = UnixStream::connect(&socket).await
        .context("connecting to broker for list_agents")?;
    write_frame_async(&mut s, &serde_json::to_vec(&Request::ListAgents)?).await?;
    let frame = read_frame_async(&mut s).await?;
    let all_agents: Vec<AgentDto> = match serde_json::from_slice::<Response>(&frame)? {
        Response::Agents { agents } => agents,
        Response::Error { message } => anyhow::bail!("list_agents failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };
    drop(s);
    let peers: Vec<&AgentDto> = all_agents.iter().filter(|a| a.name != name).collect();

    // 3. Send a welcome DM to the new agent (introduces the room + tool surface).
    let welcome_text = welcome_message(&name, kind.as_str(), &session, &peers);
    system_tell(&socket, &name, &welcome_text).await
        .with_context(|| format!("sending welcome DM to `{}`", name))?;

    // 4. Send a personal DM to each existing peer announcing the newcomer.
    let announce_text = announce_message(&name, kind.as_str());
    for peer in &peers {
        if let Err(e) = system_tell(&socket, &peer.name, &announce_text).await {
            // Best-effort — log and continue; the join still succeeded.
            eprintln!("warning: failed to announce to `{}`: {:#}", peer.name, e);
        }
    }

    // 5. Use the shared launch helper.
    let spec = Spec {
        session: session.clone(),
        name: name.clone(),
        kind,
        token,
        workdir: workdir.clone(),
        extra_args,
    };
    launch::launch_in_tmux(&spec, &socket)?;

    println!("agent `{}` ({}) added to session `{}`.", name, kind.as_str(), session);
    if !peers.is_empty() {
        let peer_names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
        println!("announced to {} existing peer(s): {}", peers.len(), peer_names.join(", "));
    }
    Ok(())
}

/// Send a `Tell` from sender "system" to `recipient` via the broker, drain the ack.
async fn system_tell(socket: &Path, recipient: &str, text: &str) -> Result<()> {
    let mut s = UnixStream::connect(socket).await?;
    let req = Request::Tell {
        from: "system".into(),
        to: Some(recipient.to_string()),
        text: text.to_string(),
        urgent: false,
    };
    write_frame_async(&mut s, &serde_json::to_vec(&req)?).await?;
    let frame = read_frame_async(&mut s).await?;
    match serde_json::from_slice::<Response>(&frame)? {
        Response::TellAck { .. } => Ok(()),
        Response::Error { message } => anyhow::bail!("tell failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    }
}

fn welcome_message(name: &str, cli_kind: &str, session: &str, peers: &[&AgentDto]) -> String {
    let peer_lines = if peers.is_empty() {
        "  (none yet)".to_string()
    } else {
        peers
            .iter()
            .map(|p| format!("  - {} ({})", p.name, p.cli_kind))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "[agents-connector] Welcome, {name}!\n\
         \n\
         You are agent \"{name}\" ({cli_kind}) in session \"{session}\".\n\
         \n\
         Other agents currently in this session:\n\
         {peer_lines}\n\
         \n\
         You can talk to peers via the `agents_connector` MCP server tools:\n\
         \u{0020}\u{0020}- tell(to, text, urgent)             \u{2014} send a message; urgent=true also pokes the recipient's tmux pane\n\
         \u{0020}\u{0020}- ask(to, text)                      \u{2014} ask a question; returns ask_id\n\
         \u{0020}\u{0020}- wait_for_reply(ask_id, timeout_ms) \u{2014} block until a reply arrives or timeout\n\
         \u{0020}\u{0020}- check_replies(ask_id)              \u{2014} non-blocking poll for replies\n\
         \u{0020}\u{0020}- read_messages(since)               \u{2014} fetch your inbox since a high-water-mark id\n\
         \u{0020}\u{0020}- post_reply(ask_id, text)           \u{2014} reply to an ask\n\
         \u{0020}\u{0020}- list_agents()                      \u{2014} see who's in the session\n\
         \n\
         When you receive a message with a non-null `ask_id`, the sender is asking you a question \u{2014} reply with `post_reply` when appropriate."
    )
}

fn announce_message(new_name: &str, new_cli_kind: &str) -> String {
    format!(
        "[agents-connector] Agent \"{new_name}\" ({new_cli_kind}) just joined this session. You can reach them via `tell(to=\"{new_name}\", ...)` or `ask(to=\"{new_name}\", ...)`."
    )
}
