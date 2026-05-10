//! Tmux send-keys wake helper.
//!
//! When an `urgent` message arrives, the broker fires this against the recipient's
//! tmux pane to nudge an idle CLI into taking a turn. Best-effort — succeeds even
//! if tmux isn't running or the target pane doesn't exist (logs and moves on).
//!
//! Set `AGENTS_CONNECTOR_DISABLE_WAKE=1` to no-op (used by tests).

use std::process::Command;

/// Send `text` followed by Enter to `<session>:<agent_name>`.
pub fn nudge(session: &str, agent_name: &str, text: &str) {
    if std::env::var("AGENTS_CONNECTOR_DISABLE_WAKE").is_ok() {
        return;
    }
    let target = format!("{}:{}", session, agent_name);
    let result = Command::new("tmux")
        .args(["send-keys", "-t", &target, text, "Enter"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match result {
        Ok(s) if s.success() => {}
        Ok(s) => tracing::warn!(target = %target, status = ?s, "tmux send-keys non-zero"),
        Err(e) => tracing::warn!(target = %target, error = %e, "tmux send-keys failed"),
    }
}
