//! Length-prefixed JSON IPC protocol.
//!
//! Wire format: 4-byte big-endian length prefix, then UTF-8 JSON body.
//! Used between the broker daemon and (a) the mcp-shim, (b) the launcher.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    /// Auth handshake from a shim — identifies the agent.
    Authenticate { agent_token: String },
    Tell { from: String, to: Option<String>, text: String, urgent: bool },
    Ask { from: String, to: String, text: String, urgent: bool },
    SetAgentState { agent_token: String, state: String },
    PostReply { from: String, ask_id: i64, text: String },
    ReadMessages { agent: String, since: i64 },
    CheckReplies { ask_id: i64 },
    /// Block until at least one reply exists, or timeout.
    WaitForReply { ask_id: i64, timeout_ms: u64 },
    ListAgents,
    /// Subscribe to live message stream (for `tail`).
    SubscribeStream,
    RegisterAgent { name: String, cli_kind: String, workdir: Option<String> },
    /// Remove an agent (soft-delete). Returns the freed token so caller can clean up files.
    RemoveAgent { name: String },
    /// Look up an agent by name. Returns full details (including token + workdir) needed for restart/resume.
    GetAgent { name: String },
    /// Graceful shutdown signal (used by `stop`).
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Ok,
    RegisterAck { agent_token: String },
    AgentInfo { name: String, cli_kind: String },
    TellAck { message_id: i64 },
    AskAck { ask_id: i64 },
    ReplyAck { reply_id: i64 },
    Messages { messages: Vec<MessageDto> },
    Replies { replies: Vec<ReplyDto> },
    Agents { agents: Vec<AgentDto> },
    /// Streamed event (one of many) for SubscribeStream.
    StreamEvent { message: MessageDto },
    /// Full agent details, used by the launcher to relaunch / restart an agent.
    AgentDetails {
        name: String,
        cli_kind: String,
        token: String,
        workdir: Option<String>,
    },
    /// Removal acknowledgment; carries the freed token.
    RemoveAck { freed_token: String },
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageDto {
    pub id: i64,
    pub from: String,
    pub to: Option<String>,
    pub text: String,
    pub ask_id: Option<i64>,
    pub in_reply_to: Option<i64>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReplyDto {
    pub id: i64,
    pub ask_id: i64,
    pub from: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentDto {
    pub name: String,
    pub cli_kind: String,
}

/// Sync framing helpers (used by the hook subcommand which is one-shot).
pub fn write_frame_sync<W: Write>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let len = (payload.len() as u32).to_be_bytes();
    w.write_all(&len)?;
    w.write_all(payload)?;
    w.flush()
}

pub fn read_frame_sync<R: Read>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Async framing helpers (used by the broker server and mcp-shim).
pub async fn write_frame_async<W: AsyncWriteExt + Unpin>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let len = (payload.len() as u32).to_be_bytes();
    w.write_all(&len).await?;
    w.write_all(payload).await?;
    w.flush().await
}

pub async fn read_frame_async<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_request() {
        let req = Request::Tell {
            from: "alice".into(),
            to: Some("bob".into()),
            text: "hi".into(),
            urgent: false,
        };
        let bytes = serde_json::to_vec(&req).unwrap();
        let parsed: Request = serde_json::from_slice(&bytes).unwrap();
        match parsed {
            Request::Tell { from, to, text, urgent } => {
                assert_eq!(from, "alice");
                assert_eq!(to.as_deref(), Some("bob"));
                assert_eq!(text, "hi");
                assert!(!urgent);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn round_trip_response() {
        let resp = Response::Messages {
            messages: vec![MessageDto {
                id: 1,
                from: "alice".into(),
                to: Some("bob".into()),
                text: "hi".into(),
                ask_id: None,
                in_reply_to: None,
                created_at: "2026-05-09T10:00:00Z".into(),
            }],
        };
        let bytes = serde_json::to_vec(&resp).unwrap();
        let parsed: Response = serde_json::from_slice(&bytes).unwrap();
        match parsed {
            Response::Messages { messages } => assert_eq!(messages.len(), 1),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn sync_framing_round_trip() {
        let mut buf = Vec::new();
        write_frame_sync(&mut buf, b"hello").unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        let frame = read_frame_sync(&mut cursor).unwrap();
        assert_eq!(frame, b"hello");
    }
}
