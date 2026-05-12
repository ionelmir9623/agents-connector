//! Tmux send-keys wake helper.
//!
//! When an `urgent` message arrives, the broker fires this against the recipient's
//! tmux pane to nudge an idle CLI into taking a turn. Best-effort — succeeds even
//! if tmux isn't running or the target pane doesn't exist (logs and moves on).
//!
//! Set `AGENTS_CONNECTOR_DISABLE_WAKE=1` to no-op (used by tests).
//!
//! Implementation note: we issue TWO `tmux send-keys` invocations — the first
//! with `-l` (literal flag) writes the text into the pane's input buffer
//! exactly as-is; the second sends the `Enter` key on its own. Combining text
//! and Enter into a single `send-keys` call works sometimes but is unreliable
//! across TUIs (the Enter can be tokenized as part of the literal payload or
//! land before the text is rendered). Two calls + a small sleep is the
//! well-known reliable pattern.

use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

/// Send `text` followed by Enter to `<session>:<agent_name>`.
pub fn nudge(session: &str, agent_name: &str, text: &str) {
    if std::env::var("AGENTS_CONNECTOR_DISABLE_WAKE").is_ok() {
        return;
    }
    let target = format!("{}:{}", session, agent_name);

    // 1. Type the literal text (no key-name interpretation, no shell parsing).
    let typed = Command::new("tmux")
        .args(["send-keys", "-l", "-t", &target, text])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match typed {
        Ok(s) if s.success() => {}
        Ok(s) => {
            tracing::warn!(target = %target, status = ?s, "tmux send-keys (text) non-zero");
            return;
        }
        Err(e) => {
            tracing::warn!(target = %target, error = %e, "tmux send-keys (text) failed");
            return;
        }
    }

    // 2. Brief pause so the TUI has a chance to render the input area before
    //    the Enter key arrives. Without this, some TUIs swallow the Enter or
    //    process it before the prompt buffer is ready.
    sleep(Duration::from_millis(50));

    // 3. Press Enter as a separate key event so the TUI submits the prompt.
    let entered = Command::new("tmux")
        .args(["send-keys", "-t", &target, "Enter"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match entered {
        Ok(s) if s.success() => {}
        Ok(s) => tracing::warn!(target = %target, status = ?s, "tmux send-keys (Enter) non-zero"),
        Err(e) => tracing::warn!(target = %target, error = %e, "tmux send-keys (Enter) failed"),
    }
}
