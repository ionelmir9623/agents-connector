//! Reusable agent-launch helper.
//!
//! Given an agent's identity (name, cli_kind, token, workdir) and the
//! session's broker socket, this regenerates the adapter config files (idempotent)
//! and spawns the CLI in a tmux window of the given session.

use crate::adapters::{claude, CliKind};
use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct Spec {
    pub session: String,
    pub name: String,
    pub kind: CliKind,
    pub token: String,
    pub workdir: Option<PathBuf>,
}

/// Generate adapter config for the agent and spawn it in a new tmux window.
/// Idempotent against the on-disk config files (overwrites with same content).
pub fn launch_in_tmux(spec: &Spec, broker_socket: &Path) -> Result<()> {
    let agent_dir = paths::session_agent_dir(&spec.session, &spec.name)?;
    let exe = std::env::current_exe()?;

    let launch_cmd = match spec.kind {
        CliKind::Claude => {
            let generated = claude::generate(&agent_dir, &exe, broker_socket, &spec.token)?;
            format!(
                "claude --mcp-config {} --settings {}",
                shell_quote(&generated.mcp_config_path.to_string_lossy()),
                shell_quote(&generated.settings_path.to_string_lossy())
            )
        }
    };

    let workdir_str = spec.workdir.as_ref().map(|p| p.to_string_lossy().to_string());
    let cd_prefix = workdir_str.as_ref()
        .map(|d| format!("cd {} && ", shell_quote(d)))
        .unwrap_or_default();
    let full_cmd = format!("{}{}", cd_prefix, launch_cmd);

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
