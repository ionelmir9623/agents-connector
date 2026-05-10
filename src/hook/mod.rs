//! Hook subcommand: runs at end-of-turn (or other adapter event), checks for new
//! messages, emits CLI-specific JSON to inject them as additional context.

use crate::ipc::{read_frame_sync, write_frame_sync, MessageDto, Request, Response};
use anyhow::{Context, Result};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub fn run(socket: PathBuf, agent_token: String, event: String, cli_kind: String) -> Result<()> {
    diag_log(&socket, &cli_kind, &event, "invoked");

    if !injects_context(&cli_kind, &event) {
        diag_log(&socket, &cli_kind, &event, "no-op (cli_kind/event combo not injectable)");
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

    diag_log(
        &socket,
        &cli_kind,
        &event,
        &format!("agent={} since={} new_msgs={}", agent_name, since, msgs.len()),
    );

    if msgs.is_empty() {
        return Ok(());
    }

    if let Some(last) = msgs.last() {
        std::fs::write(&hwm_file, last.id.to_string())?;
    }

    let text = format_messages(&msgs);
    let payload = format_payload(&cli_kind, &event, &text);
    diag_log(&socket, &cli_kind, &event, &format!("emitting payload: {}", payload));
    println!("{}", payload);
    Ok(())
}

/// Append a diagnostic line to `<session_dir>/hook_log.txt` for debugging
/// whether and when adapter hooks fire. Best-effort — failures are silently
/// ignored so they never break the host CLI.
fn diag_log(socket: &std::path::Path, cli_kind: &str, event: &str, note: &str) {
    let Some(session_dir) = socket.parent() else { return };
    let log_path = session_dir.join("hook_log.txt");
    let ts = chrono::Utc::now().to_rfc3339();
    let line = format!("{ts} cli_kind={cli_kind} event={event} :: {note}\n");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
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

fn format_payload(cli_kind: &str, event: &str, text: &str) -> serde_json::Value {
    match cli_kind {
        // Claude Code: flat additionalContext.
        "claude" => serde_json::json!({ "additionalContext": text }),
        // Codex: nested hookSpecificOutput with required hookEventName field.
        // Schema per https://developers.openai.com/codex/hooks
        "codex" => {
            let hook_event_name = codex_event_name(event);
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": hook_event_name,
                    "additionalContext": text,
                }
            })
        }
        _ => serde_json::json!({}),
    }
}

/// Map our snake_case --event values to Codex's PascalCase hookEventName values.
fn codex_event_name(event: &str) -> &'static str {
    match event {
        "post_tool_use" => "PostToolUse",
        "user_prompt_submit" => "UserPromptSubmit",
        "session_start" => "SessionStart",
        "stop" => "Stop",
        // Unknown events: pass through with first-letter-uppercase as a best guess.
        // Codex will error if it doesn't match an event it knows.
        _ => "Unknown",
    }
}
