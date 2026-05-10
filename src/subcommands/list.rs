use crate::paths;
use anyhow::Result;
use std::fs;

pub fn run() -> Result<()> {
    let dir = paths::sessions_dir()?;
    if !dir.exists() {
        println!("no sessions yet.");
        return Ok(());
    }
    let mut sessions: Vec<String> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    sessions.sort();

    if sessions.is_empty() {
        println!("no sessions yet.");
        return Ok(());
    }

    println!("{:<20} {:<10} {:<10}", "NAME", "STATUS", "PID");
    for s in sessions {
        let pid_file = paths::session_pid_file(&s)?;
        let (status, pid) = match fs::read_to_string(&pid_file) {
            Ok(p) => {
                let pid: i32 = p.trim().parse().unwrap_or(0);
                if pid > 0 && process_alive(pid) {
                    ("running".to_string(), p.trim().to_string())
                } else {
                    ("stopped".to_string(), "—".to_string())
                }
            }
            Err(_) => ("stopped".to_string(), "—".to_string()),
        };
        println!("{:<20} {:<10} {:<10}", s, status, pid);
    }
    Ok(())
}

fn process_alive(pid: i32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid), None).is_ok()
}
