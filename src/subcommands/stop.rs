use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::net::UnixStream;

pub async fn run(session: &str, kill_tmux: bool) -> Result<()> {
    let socket = paths::session_socket(session)?;
    if !socket.exists() {
        println!("session `{}` not running.", session);
    } else {
        // Send Shutdown over IPC.
        let mut s = UnixStream::connect(&socket).await
            .with_context(|| format!("connecting to broker for session `{}`", session))?;
        write_frame_async(&mut s, &serde_json::to_vec(&Request::Shutdown)?).await?;
        let frame = read_frame_async(&mut s).await?;
        let _: Response = serde_json::from_slice(&frame)?;

        // Wait for socket to disappear.
        for _ in 0..200 {
            if !socket.exists() { break; }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    let pid_file = paths::session_pid_file(session)?;
    if pid_file.exists() {
        std::fs::remove_file(&pid_file)?;
    }

    if kill_tmux && tmux::has_session(session)? {
        tmux::kill_session(session)?;
    }

    println!("session `{}` stopped.", session);
    Ok(())
}
