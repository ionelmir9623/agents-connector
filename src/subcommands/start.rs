use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn run(session: &str, workdir: Option<PathBuf>) -> Result<()> {
    let session_dir = paths::session_dir(session)?;
    if session_dir.exists() {
        anyhow::bail!(
            "session `{}` already exists at {}\n\
             Use `agents-connector resume {}` to bring it back, or pick a different name.",
            session, session_dir.display(), session
        );
    }
    if tmux::has_session(session)? {
        anyhow::bail!(
            "tmux session `{}` already exists. Pick a different name or kill the existing tmux session.",
            session
        );
    }
    std::fs::create_dir_all(&session_dir)?;

    let db = paths::session_db(session)?;
    let socket = paths::session_socket(session)?;
    let pid_file = paths::session_pid_file(session)?;
    let log = paths::session_log(session)?;

    // Spawn the broker daemon detached.
    let exe = std::env::current_exe()?;
    let log_file = std::fs::File::create(&log)?;
    let log_file_err = log_file.try_clone()?;
    let child = Command::new(&exe)
        .args([
            "broker",
            "--socket", &socket.to_string_lossy(),
            "--db", &db.to_string_lossy(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .spawn()
        .context("spawning broker daemon")?;

    std::fs::write(&pid_file, child.id().to_string())?;

    // Wait for the socket to appear (broker is up).
    let mut ok = false;
    for _ in 0..200 {
        if socket.exists() { ok = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    if !ok {
        anyhow::bail!("broker failed to start within 5 seconds; check {}", log.display());
    }

    // Create tmux session with a tail pane.
    tmux::new_detached_session(session, workdir.as_ref())?;
    let tail_command = format!(
        "{} tail {}",
        exe.to_string_lossy(),
        session
    );
    tmux::split_window_below(session, 25, &tail_command)?;

    println!("session `{}` started.", session);
    println!("attach with: agents-connector attach {}", session);

    // Attach the user's terminal.
    tmux::attach_session(session)?;
    Ok(())
}
