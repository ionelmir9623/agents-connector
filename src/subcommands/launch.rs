//! Reusable agent-launch helper.
//!
//! Given an agent's identity (name, cli_kind, token, workdir) and the
//! session's broker socket, this regenerates the adapter config files (idempotent)
//! and spawns the CLI in a tmux window of the given session.

use crate::adapters::{claude, codex, gemini, CliKind};
use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct Spec {
    pub session: String,
    pub name: String,
    pub kind: CliKind,
    pub token: String,
    pub workdir: Option<PathBuf>,
    pub extra_args: Vec<String>,
}

/// Generate adapter config for the agent and spawn it in a new tmux window.
/// Idempotent against the on-disk config files (overwrites with same content).
pub fn launch_in_tmux(spec: &Spec, broker_socket: &Path) -> Result<()> {
    let agent_dir = paths::session_agent_dir(&spec.session, &spec.name)?;
    let exe = std::env::current_exe()?;

    let full_cmd = match spec.kind {
        CliKind::Claude => {
            let generated = claude::generate(&agent_dir, &exe, broker_socket, &spec.token)?;
            let mut claude_cmd = format!(
                "claude --mcp-config {} --settings {}",
                shell_quote(&generated.mcp_config_path.to_string_lossy()),
                shell_quote(&generated.settings_path.to_string_lossy())
            );
            for arg in &spec.extra_args {
                claude_cmd.push(' ');
                claude_cmd.push_str(&shell_quote(arg));
            }
            // Claude has no --cd; use shell `cd && ` if workdir is set.
            match spec.workdir.as_ref() {
                Some(dir) => format!("cd {} && {}", shell_quote(&dir.to_string_lossy()), claude_cmd),
                None => claude_cmd,
            }
        }
        CliKind::Codex => {
            let overrides = codex::config_overrides(&exe, broker_socket, &spec.token);
            let mut parts: Vec<String> = vec!["codex".into()];
            for ov in &overrides {
                parts.push("-c".into());
                parts.push(shell_quote(ov));
            }
            if let Some(dir) = spec.workdir.as_ref() {
                parts.push("--cd".into());
                parts.push(shell_quote(&dir.to_string_lossy()));
            }
            for arg in &spec.extra_args {
                parts.push(shell_quote(arg));
            }
            parts.join(" ")
        }
        CliKind::Gemini => {
            // Gemini reads .gemini/settings.json from cwd. We write it to agent_dir
            // and cd into agent_dir before launching. The user's project workdir (if any)
            // is added via --include-directories so gemini can read/write there.
            let generated = gemini::generate(&agent_dir, &exe, broker_socket, &spec.token)?;
            let mut parts: Vec<String> = vec![
                format!("cd {} &&", shell_quote(&generated.launch_cwd.to_string_lossy())),
                "gemini".into(),
            ];
            if let Some(dir) = spec.workdir.as_ref() {
                parts.push("--include-directories".into());
                parts.push(shell_quote(&dir.to_string_lossy()));
            }
            for arg in &spec.extra_args {
                parts.push(shell_quote(arg));
            }
            parts.join(" ")
        }
    };

    tmux::new_window(&spec.session, &spec.name, &[], &full_cmd)
        .with_context(|| format!("spawning tmux window for agent `{}`", spec.name))?;
    Ok(())
}

/// Kill a tmux window by `<session>:<agent_name>` target. Idempotent — succeeds even
/// if the window is already gone (returns Ok). Errors only on tmux invocation failure.
pub fn kill_agent_window(session: &str, agent_name: &str) -> Result<()> {
    let target = format!("{}:{}", session, agent_name);
    let _ = std::process::Command::new("tmux")
        .args(["kill-window", "-t", &target])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("running `tmux kill-window`")?;
    Ok(())
}

pub fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "_-./=:".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}
