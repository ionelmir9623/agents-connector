//! Thin wrappers around tmux CLI commands.

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn has_session(name: &str) -> Result<bool> {
    let status = Command::new("tmux")
        .args(["has-session", "-t", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("running `tmux has-session`")?;
    Ok(status.success())
}

pub fn new_detached_session(name: &str, workdir: Option<&PathBuf>) -> Result<()> {
    let mut cmd = Command::new("tmux");
    cmd.args(["new-session", "-d", "-s", name]);
    if let Some(d) = workdir {
        cmd.args(["-c", &d.to_string_lossy()]);
    }
    let status = cmd.status().context("running `tmux new-session`")?;
    if !status.success() {
        anyhow::bail!("tmux new-session failed");
    }
    Ok(())
}

pub fn split_window_below(session: &str, percent: u32, command: &str) -> Result<()> {
    let target = format!("{}:0", session);
    // -d: do not move focus into the new (bottom) pane; user's cursor stays on the top pane.
    let status = Command::new("tmux")
        .args(["split-window", "-d", "-t", &target, "-v", "-p", &percent.to_string(), command])
        .status()
        .context("running `tmux split-window`")?;
    if !status.success() {
        anyhow::bail!("tmux split-window failed");
    }
    Ok(())
}

pub fn new_window(session: &str, name: &str, env: &[(&str, &str)], command: &str) -> Result<()> {
    let mut cmd = Command::new("tmux");
    cmd.args(["new-window", "-t", session, "-n", name]);
    for (k, v) in env {
        cmd.args(["-e", &format!("{}={}", k, v)]);
    }
    cmd.arg(command);
    let status = cmd.status().context("running `tmux new-window`")?;
    if !status.success() {
        anyhow::bail!("tmux new-window failed");
    }
    Ok(())
}

pub fn kill_session(name: &str) -> Result<()> {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .status()
        .context("running `tmux kill-session`")?;
    Ok(())
}

pub fn attach_session(name: &str) -> Result<()> {
    // Replace current process with tmux attach; this never returns on success.
    use std::os::unix::process::CommandExt;
    let err = Command::new("tmux").args(["attach-session", "-t", name]).exec();
    Err(anyhow::Error::from(err))
}

/// Returns the current tmux session name from $TMUX, if running inside tmux.
pub fn current_session() -> Option<String> {
    let tmux = std::env::var("TMUX").ok()?;
    let _ = tmux;
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#S"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}
