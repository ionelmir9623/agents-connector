use crate::adapters::CliKind;
use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::subcommands::launch::{self, Spec};
use crate::{paths, tmux};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tokio::net::UnixStream;

pub async fn run(session: &str, no_agents: bool) -> Result<()> {
    let session_dir = paths::session_dir(session)?;
    if !session_dir.exists() {
        anyhow::bail!(
            "no session `{}` to resume. Run `agents-connector start {}` to create a new one.",
            session, session
        );
    }
    let db = paths::session_db(session)?;
    if !db.exists() {
        anyhow::bail!(
            "session `{}` exists but db is missing at {}. Recovery is manual.",
            session, db.display()
        );
    }
    if tmux::has_session(session)? {
        anyhow::bail!(
            "tmux session `{}` is still alive. Use `agents-connector attach {}` instead.",
            session, session
        );
    }

    // Spawn broker daemon (mirrors start.rs).
    let socket = paths::session_socket(session)?;
    let pid_file = paths::session_pid_file(session)?;
    let log = paths::session_log(session)?;
    let exe = std::env::current_exe()?;
    let log_file = std::fs::File::create(&log)?;
    let log_file_err = log_file.try_clone()?;
    let child = Command::new(&exe)
        .args([
            "broker",
            "--socket", &socket.to_string_lossy(),
            "--db", &db.to_string_lossy(),
            "--session", session,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .spawn()
        .context("spawning broker daemon")?;
    std::fs::write(&pid_file, child.id().to_string())?;

    // Wait for the socket to appear.
    let mut ok = false;
    for _ in 0..200 {
        if socket.exists() { ok = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    if !ok {
        anyhow::bail!("broker failed to restart within 5 seconds; check {}", log.display());
    }

    // Recreate tmux session with tail pane.
    tmux::new_detached_session(session, None)?;
    let tail_command = format!("{} tail {}", exe.to_string_lossy(), session);
    tmux::split_window_below(session, 25, &tail_command)?;

    if !no_agents {
        // Ask broker for the active agent list and relaunch each.
        let mut s = UnixStream::connect(&socket).await
            .context("connecting to broker for relaunch")?;
        write_frame_async(&mut s, &serde_json::to_vec(&Request::ListAgents)?).await?;
        let frame = read_frame_async(&mut s).await?;
        let agents = match serde_json::from_slice::<Response>(&frame)? {
            Response::Agents { agents } => agents,
            other => anyhow::bail!("unexpected list_agents response: {:?}", other),
        };
        drop(s);

        for a in agents {
            // Need full details (token + workdir) to relaunch.
            let mut s = UnixStream::connect(&socket).await?;
            write_frame_async(&mut s, &serde_json::to_vec(&Request::GetAgent { name: a.name.clone() })?).await?;
            let frame = read_frame_async(&mut s).await?;
            let (token, workdir) = match serde_json::from_slice::<Response>(&frame)? {
                Response::AgentDetails { token, workdir, .. } => (token, workdir),
                Response::Error { message } => {
                    eprintln!("warning: get_agent({}) failed: {}", a.name, message);
                    continue;
                }
                other => anyhow::bail!("unexpected: {:?}", other),
            };
            drop(s);

            let kind = match CliKind::parse(&a.cli_kind) {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("warning: skipping `{}` (unsupported cli_kind `{}`): {}", a.name, a.cli_kind, e);
                    continue;
                }
            };
            let spec = Spec {
                session: session.to_string(),
                name: a.name.clone(),
                kind,
                token,
                workdir: workdir.map(PathBuf::from),
            };
            if let Err(e) = launch::launch_in_tmux(&spec, &socket) {
                eprintln!("warning: relaunch `{}` failed: {:#}", a.name, e);
            } else {
                println!("relaunched `{}` ({})", a.name, a.cli_kind);
            }
        }
    }

    println!("session `{}` resumed.", session);
    println!("attach with: agents-connector attach {}", session);

    tmux::attach_session(session)?;
    Ok(())
}
