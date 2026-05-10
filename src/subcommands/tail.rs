use crate::ipc::{read_frame_async, write_frame_async, Request, Response};
use crate::{paths, tmux};
use anyhow::{anyhow, Result};
use tokio::net::UnixStream;

pub async fn run(session: Option<String>) -> Result<()> {
    let session = session
        .or_else(tmux::current_session)
        .ok_or_else(|| anyhow!("no session specified and not inside tmux"))?;
    let socket = paths::session_socket(&session)?;
    if !socket.exists() {
        anyhow::bail!("session `{}` not running.", session);
    }

    let mut s = UnixStream::connect(&socket).await?;
    write_frame_async(&mut s, &serde_json::to_vec(&Request::SubscribeStream)?).await?;

    // Expect an initial Ok ack.
    let frame = read_frame_async(&mut s).await?;
    match serde_json::from_slice::<Response>(&frame)? {
        Response::Ok => {}
        other => anyhow::bail!("unexpected subscribe response: {:?}", other),
    }

    println!("Conversation window for `{}`", session);
    println!("Add agents with `agents-connector add ...` in the top pane");
    println!();
    loop {
        let frame = match read_frame_async(&mut s).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        let resp: Response = serde_json::from_slice(&frame)?;
        if let Response::StreamEvent { message } = resp {
            let to = message.to.clone().unwrap_or_else(|| "@everyone".into());
            // created_at is RFC3339: "2026-05-09T10:00:00+00:00" — slice [11..19] = "HH:MM:SS"
            let time_part = if message.created_at.len() >= 19 {
                &message.created_at[11..19]
            } else {
                &message.created_at
            };
            println!(
                "{}  {:>10} \u{2192} {:<10}  {}",
                time_part, message.from, to, message.text
            );
        }
    }
    Ok(())
}
