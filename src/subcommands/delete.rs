//! Delete a session entirely: stop broker, kill tmux, remove filesystem state.
//!
//! `agents-connector delete <session>` deletes one session.
//! `agents-connector delete --all` deletes every session on this machine.

use crate::ipc::{read_frame_async, write_frame_async, Request};
use crate::{paths, tmux};
use anyhow::{anyhow, Context, Result};
use std::time::Duration;
use tokio::net::UnixStream;

pub async fn run(session: Option<String>, all: bool) -> Result<()> {
    if all {
        delete_all().await
    } else {
        let name = session.ok_or_else(|| {
            anyhow!("expected a session name or --all (e.g. `agents-connector delete demo` or `agents-connector delete --all`)")
        })?;
        delete_one(&name).await
    }
}

async fn delete_all() -> Result<()> {
    let sessions_dir = paths::sessions_dir()?;
    if !sessions_dir.exists() {
        println!("no sessions to delete.");
        return Ok(());
    }

    let mut names: Vec<String> = std::fs::read_dir(&sessions_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();

    if names.is_empty() {
        println!("no sessions to delete.");
        return Ok(());
    }

    let mut errors = 0u32;
    for name in &names {
        if let Err(e) = delete_one(name).await {
            eprintln!("warning: failed to delete `{}`: {:#}", name, e);
            errors += 1;
        }
    }

    if errors > 0 {
        anyhow::bail!("deleted with {} error(s); see warnings above", errors);
    }
    Ok(())
}

async fn delete_one(session: &str) -> Result<()> {
    let session_dir = paths::session_dir(session)?;
    if !session_dir.exists() {
        anyhow::bail!("session `{}` does not exist.", session);
    }

    // 1. Gracefully stop the broker if it's running (socket exists). Best-effort.
    let socket = paths::session_socket(session)?;
    if socket.exists() {
        if let Ok(mut s) = UnixStream::connect(&socket).await {
            let _ = write_frame_async(&mut s, &serde_json::to_vec(&Request::Shutdown)?).await;
            // Read the Ack frame; ignore any error.
            let _ = read_frame_async(&mut s).await;
            // Wait briefly for the broker to flush and remove the socket file.
            for _ in 0..50 {
                if !socket.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    // 2. Kill the tmux session if it exists. Idempotent.
    if tmux::has_session(session).unwrap_or(false) {
        let _ = tmux::kill_session(session);
    }

    // 3. Remove the filesystem state — recursive, includes DB + agent dirs + logs.
    std::fs::remove_dir_all(&session_dir)
        .with_context(|| format!("removing {}", session_dir.display()))?;

    println!("deleted session `{}`.", session);
    Ok(())
}
