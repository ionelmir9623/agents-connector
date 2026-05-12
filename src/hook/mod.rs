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
        // Claude Code: Stop CANNOT inject (only decision:"block"). Use the
        // hookSpecificOutput-supporting events instead.
        ("claude", "user_prompt_submit") => true,
        ("claude", "post_tool_use") => true,
        ("claude", "session_start") => true,
        ("codex", "post_tool_use") => true,
        ("codex", "user_prompt_submit") => true,
        ("gemini", "before_agent") => true,
        ("gemini", "after_tool") => true,
        ("gemini", "session_start") => true,
        _ => false,
    }
}

fn format_messages(msgs: &[MessageDto]) -> String {
    let has_ask = msgs.iter().any(|m| m.ask_id.is_some());
    let mut text = if has_ask {
        String::from("[agents-connector] You have new messages, including an incoming ask. Address asks via the `post_reply` tool when appropriate.\n\n")
    } else {
        String::from("[agents-connector] You have new messages:\n\n")
    };
    for m in msgs {
        let to = m.to.as_deref().unwrap_or("@everyone");
        match m.ask_id {
            Some(ask_id) => text.push_str(&format!(
                "ASK from `{}` (ask_id={}): {}\n  \u{2192} reply with `post_reply(ask_id={}, text=...)`\n",
                m.from, ask_id, m.text, ask_id,
            )),
            None => text.push_str(&format!(
                "MESSAGE from `{}` \u{2192} `{}`: {}\n",
                m.from, to, m.text,
            )),
        }
    }
    text.push_str("\nFetch later messages with `read_messages(since=N)`, or use `tell`/`ask` to initiate.");
    text
}

fn format_payload(cli_kind: &str, event: &str, text: &str) -> serde_json::Value {
    match cli_kind {
        // Claude Code: nested under hookSpecificOutput with hookEventName.
        // Schema per https://code.claude.com/docs/en/hooks.
        "claude" => {
            let hook_event_name = claude_event_name(event);
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": hook_event_name,
                    "additionalContext": text,
                }
            })
        }
        // Codex: nested under hookSpecificOutput, requires hookEventName.
        "codex" => {
            let hook_event_name = codex_event_name(event);
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": hook_event_name,
                    "additionalContext": text,
                }
            })
        }
        // Gemini CLI: nested under hookSpecificOutput, NO hookEventName field.
        "gemini" => serde_json::json!({
            "hookSpecificOutput": {
                "additionalContext": text,
            }
        }),
        _ => serde_json::json!({}),
    }
}

/// Map our snake_case --event values to Claude Code's PascalCase hookEventName values.
fn claude_event_name(event: &str) -> &'static str {
    match event {
        "user_prompt_submit" => "UserPromptSubmit",
        "post_tool_use" => "PostToolUse",
        "session_start" => "SessionStart",
        "pre_tool_use" => "PreToolUse",
        "user_prompt_expansion" => "UserPromptExpansion",
        _ => "Unknown",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_payload_omits_hook_event_name() {
        let payload = format_payload("gemini", "before_agent", "hello");
        assert_eq!(payload["hookSpecificOutput"]["additionalContext"], "hello");
        assert!(payload["hookSpecificOutput"]["hookEventName"].is_null());
    }

    #[test]
    fn codex_payload_includes_hook_event_name() {
        let payload = format_payload("codex", "post_tool_use", "hello");
        assert_eq!(payload["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert_eq!(payload["hookSpecificOutput"]["additionalContext"], "hello");
    }

    #[test]
    fn claude_payload_is_nested_with_hook_event_name() {
        // Claude's Stop hook can't inject, so we use UserPromptSubmit/PostToolUse/SessionStart.
        let payload = format_payload("claude", "user_prompt_submit", "hello");
        assert_eq!(payload["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
        assert_eq!(payload["hookSpecificOutput"]["additionalContext"], "hello");
        assert!(payload["additionalContext"].is_null());
    }

    #[test]
    fn claude_stop_event_is_not_injectable() {
        // Stop can't inject context — only decision:"block". Confirm we treat it as no-op.
        assert!(!injects_context("claude", "stop"));
    }

    #[test]
    fn claude_session_start_injects() {
        assert!(injects_context("claude", "session_start"));
        let payload = format_payload("claude", "session_start", "hi");
        assert_eq!(payload["hookSpecificOutput"]["hookEventName"], "SessionStart");
    }

    #[test]
    fn gemini_before_agent_injects() {
        assert!(injects_context("gemini", "before_agent"));
        assert!(injects_context("gemini", "after_tool"));
    }

    #[test]
    fn gemini_after_agent_does_not_inject() {
        // AfterAgent CANNOT inject context per gemini docs.
        assert!(!injects_context("gemini", "after_agent"));
    }

    #[test]
    fn format_messages_calls_out_asks() {
        let msgs = vec![MessageDto {
            id: 1,
            from: "writer".into(),
            to: Some("reviewer".into()),
            text: "review this".into(),
            ask_id: Some(42),
            in_reply_to: None,
            created_at: "2026-05-11T00:00:00+00:00".into(),
        }];
        let out = format_messages(&msgs);
        assert!(out.contains("incoming ask"));
        assert!(out.contains("ASK from `writer`"));
        assert!(out.contains("ask_id=42"));
        assert!(out.contains("post_reply"));
    }

    #[test]
    fn format_messages_plain_tell_no_ask_callout() {
        let msgs = vec![MessageDto {
            id: 1,
            from: "writer".into(),
            to: Some("reviewer".into()),
            text: "fyi".into(),
            ask_id: None,
            in_reply_to: None,
            created_at: "2026-05-11T00:00:00+00:00".into(),
        }];
        let out = format_messages(&msgs);
        assert!(!out.contains("incoming ask"));
        assert!(out.contains("MESSAGE from `writer`"));
    }
}
