//! Hook subcommand: runs at end-of-turn (or other adapter event), checks for new
//! messages, emits CLI-specific JSON to inject them as additional context.

use crate::ipc::{read_frame_sync, write_frame_sync, MessageDto, Request, Response};
use anyhow::{Context, Result};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub fn run(socket: PathBuf, agent_token: String, event: String, cli_kind: String) -> Result<()> {
    if !injects_context(&cli_kind, &event) {
        return Ok(());
    }

    let mut stream = UnixStream::connect(&socket)
        .with_context(|| format!("connecting to broker at {}", socket.display()))?;

    // Authenticate.
    let req = Request::Authenticate { agent_token: agent_token.clone() };
    write_frame_sync(&mut stream, &serde_json::to_vec(&req)?)?;
    let frame = read_frame_sync(&mut stream)?;
    let agent_name = match serde_json::from_slice::<Response>(&frame)? {
        Response::AgentInfo { name, .. } => name,
        Response::Error { message } => anyhow::bail!("auth failed: {}", message),
        other => anyhow::bail!("unexpected: {:?}", other),
    };

    // Read messages since high-water-mark.
    let session_dir = socket.parent()
        .ok_or_else(|| anyhow::anyhow!("malformed socket path"))?;
    let agent_dir = session_dir.join("agents").join(&agent_name);
    std::fs::create_dir_all(&agent_dir)?;
    let hwm_file = agent_dir.join("last_seen_message_id");
    let since: i64 = std::fs::read_to_string(&hwm_file).ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    let req = Request::ReadMessages { agent: agent_name.clone(), since };
    write_frame_sync(&mut stream, &serde_json::to_vec(&req)?)?;
    let frame = read_frame_sync(&mut stream)?;
    let msgs = match serde_json::from_slice::<Response>(&frame)? {
        Response::Messages { messages } => messages,
        other => anyhow::bail!("unexpected: {:?}", other),
    };

    if msgs.is_empty() {
        return Ok(());
    }

    if let Some(last) = msgs.last() {
        std::fs::write(&hwm_file, last.id.to_string())?;
    }

    let text = format_messages(&msgs);
    let payload = format_payload(&cli_kind, &text);
    println!("{}", payload);
    Ok(())
}

fn injects_context(cli_kind: &str, event: &str) -> bool {
    match (cli_kind, event) {
        ("claude", "stop") => true,
        ("codex", "post_tool_use") => true,
        ("codex", "user_prompt_submit") => true,
        _ => false,
    }
}

fn format_messages(msgs: &[MessageDto]) -> String {
    let mut text = String::from("[agents-connector] You have new messages:\n");
    for m in msgs {
        let to = m.to.as_deref().unwrap_or("@everyone");
        text.push_str(&format!("- from {} → {}: {}\n", m.from, to, m.text));
    }
    text.push_str("\nUse the `read_messages` MCP tool with `since` set to the latest id you've handled to retrieve again, or use `tell`/`ask` to respond.");
    text
}

fn format_payload(cli_kind: &str, text: &str) -> serde_json::Value {
    match cli_kind {
        "claude" => serde_json::json!({ "additionalContext": text }),
        "codex" => serde_json::json!({
            "hookSpecificOutput": { "additionalContext": text }
        }),
        _ => serde_json::json!({}),
    }
}
