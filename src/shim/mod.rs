//! MCP shim: stdio MCP server that bridges to the broker via Unix IPC.
//!
//! Architecture: the agent's CLI (e.g. Claude Code) launches `agents-connector
//! mcp-shim --socket … --agent-token …` as a child and speaks MCP over its
//! stdin/stdout. This module is that child: it terminates MCP, then for every
//! tool call it issues one IPC request to the broker and returns the response
//! back over MCP. It also auto-injects the canonical agent name (learned during
//! `Authenticate`) into every request that needs it, so the CLI never has to
//! know what its broker-side identity is.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::net::UnixStream;
use tokio::sync::Mutex;

use crate::ipc::{
    read_frame_async, write_frame_async, AgentDto, MessageDto, ReplyDto, Request, Response,
};

// ---------------------------------------------------------------------------
// BrokerClient — owns the IPC socket; serialises concurrent requests behind a
// mutex (the protocol is strict request/response, not multiplexed).
// ---------------------------------------------------------------------------

pub struct BrokerClient {
    stream: Mutex<UnixStream>,
    pub agent_name: String,
}

impl BrokerClient {
    pub async fn connect(socket: &PathBuf, agent_token: &str) -> Result<Self> {
        let mut stream = UnixStream::connect(socket)
            .await
            .with_context(|| format!("connecting to broker at {}", socket.display()))?;

        let req = Request::Authenticate {
            agent_token: agent_token.to_string(),
        };
        write_frame_async(&mut stream, &serde_json::to_vec(&req)?).await?;
        let frame = read_frame_async(&mut stream).await?;
        let resp: Response = serde_json::from_slice(&frame)?;
        let agent_name = match resp {
            Response::AgentInfo { name, .. } => name,
            Response::Error { message } => return Err(anyhow!("auth failed: {}", message)),
            other => return Err(anyhow!("unexpected auth response: {:?}", other)),
        };

        Ok(Self {
            stream: Mutex::new(stream),
            agent_name,
        })
    }

    pub async fn request(&self, req: Request) -> Result<Response> {
        let mut s = self.stream.lock().await;
        write_frame_async(&mut *s, &serde_json::to_vec(&req)?).await?;
        let frame = read_frame_async(&mut *s).await?;
        let resp: Response = serde_json::from_slice(&frame)?;
        Ok(resp)
    }
}

// ---------------------------------------------------------------------------
// Tool argument structs.
//
// rmcp 1.6's `#[tool]` macro extracts the call's `arguments` JSON object via a
// single `Parameters<T>` parameter — there's no positional-arg form. So each
// tool gets a small `*Args` struct here that mirrors the table in the plan.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TellArgs {
    /// Recipient agent name. Omit to broadcast to the room.
    #[serde(default)]
    pub to: Option<String>,
    /// Message body.
    pub text: String,
    /// Mark as urgent (adapter may surface this differently).
    #[serde(default)]
    pub urgent: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AskArgs {
    /// Recipient agent name (required for ask).
    pub to: String,
    /// Question body.
    pub text: String,
    /// If true (default), auto-wake the recipient if they are idle.
    #[serde(default)]
    pub urgent: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct WaitForReplyArgs {
    pub ask_id: i64,
    pub timeout_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CheckRepliesArgs {
    pub ask_id: i64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ReadMessagesArgs {
    /// Return messages whose id is strictly greater than `since`.
    pub since: i64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PostReplyArgs {
    pub ask_id: i64,
    pub text: String,
}

// ---------------------------------------------------------------------------
// Shim — the MCP service. Each tool is one IPC round-trip.
//
// Tool methods return `Result<String, ErrorData>`:
//   * scalar acks are serialised as JSON ints (e.g. `42`);
//   * collection responses are serialised as JSON arrays of the IPC DTOs.
// Returning a String avoids requiring `JsonSchema` derives on the IPC DTOs.
// The MCP client just gets text content with a JSON payload inside.
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Shim {
    client: Arc<BrokerClient>,
    tool_router: ToolRouter<Self>,
}

impl Shim {
    pub fn new(client: Arc<BrokerClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

fn ipc_err<E: std::fmt::Display>(e: E) -> ErrorData {
    ErrorData::internal_error(format!("broker IPC: {}", e), None)
}

fn unexpected(resp: Response) -> ErrorData {
    ErrorData::internal_error(format!("unexpected broker response: {:?}", resp), None)
}

fn to_json_string<T: Serialize>(value: &T) -> Result<String, ErrorData> {
    serde_json::to_string(value)
        .map_err(|e| ErrorData::internal_error(format!("serialize result: {}", e), None))
}

#[tool_router(router = tool_router)]
impl Shim {
    /// Send a message to a peer (or broadcast). Returns the new message id.
    #[tool(
        description = "Send a chat message to another agent (or broadcast if `to` is omitted). Returns the new message id."
    )]
    async fn tell(&self, Parameters(args): Parameters<TellArgs>) -> Result<String, ErrorData> {
        let req = Request::Tell {
            from: self.client.agent_name.clone(),
            to: args.to,
            text: args.text,
            urgent: args.urgent.unwrap_or(false),
        };
        match self.client.request(req).await.map_err(ipc_err)? {
            Response::TellAck { message_id } => Ok(message_id.to_string()),
            Response::Error { message } => {
                Err(ErrorData::internal_error(message, None))
            }
            other => Err(unexpected(other)),
        }
    }

    /// Ask a peer a question; returns an ask id you can pass to wait_for_reply / check_replies.
    /// By default this auto-wakes the recipient if they are idle (urgent=true).
    #[tool(description = "Ask another agent a question. Returns the ask id. Auto-wakes the recipient if idle (set urgent=false to suppress).")]
    async fn ask(&self, Parameters(args): Parameters<AskArgs>) -> Result<String, ErrorData> {
        let req = Request::Ask {
            from: self.client.agent_name.clone(),
            to: args.to,
            text: args.text,
            urgent: args.urgent.unwrap_or(true),
        };
        match self.client.request(req).await.map_err(ipc_err)? {
            Response::AskAck { ask_id } => Ok(ask_id.to_string()),
            Response::Error { message } => {
                Err(ErrorData::internal_error(message, None))
            }
            other => Err(unexpected(other)),
        }
    }

    /// Block until at least one reply for `ask_id` exists, or `timeout_ms` elapses.
    #[tool(
        description = "Block until at least one reply for the given ask id arrives, or the timeout (ms) elapses. Returns a JSON array of replies."
    )]
    async fn wait_for_reply(
        &self,
        Parameters(args): Parameters<WaitForReplyArgs>,
    ) -> Result<String, ErrorData> {
        let req = Request::WaitForReply {
            ask_id: args.ask_id,
            timeout_ms: args.timeout_ms,
        };
        match self.client.request(req).await.map_err(ipc_err)? {
            Response::Replies { replies } => to_json_string::<Vec<ReplyDto>>(&replies),
            Response::Error { message } => {
                Err(ErrorData::internal_error(message, None))
            }
            other => Err(unexpected(other)),
        }
    }

    /// Non-blocking poll for replies to `ask_id`.
    #[tool(description = "Return all replies posted to the given ask id (non-blocking).")]
    async fn check_replies(
        &self,
        Parameters(args): Parameters<CheckRepliesArgs>,
    ) -> Result<String, ErrorData> {
        let req = Request::CheckReplies {
            ask_id: args.ask_id,
        };
        match self.client.request(req).await.map_err(ipc_err)? {
            Response::Replies { replies } => to_json_string::<Vec<ReplyDto>>(&replies),
            Response::Error { message } => {
                Err(ErrorData::internal_error(message, None))
            }
            other => Err(unexpected(other)),
        }
    }

    /// Read messages addressed to (or broadcast at) this agent with id > `since`.
    #[tool(
        description = "Read messages addressed to this agent (and broadcasts) with id strictly greater than `since`. Returns a JSON array of MessageDto."
    )]
    async fn read_messages(
        &self,
        Parameters(args): Parameters<ReadMessagesArgs>,
    ) -> Result<String, ErrorData> {
        let req = Request::ReadMessages {
            agent: self.client.agent_name.clone(),
            since: args.since,
        };
        match self.client.request(req).await.map_err(ipc_err)? {
            Response::Messages { messages } => to_json_string::<Vec<MessageDto>>(&messages),
            Response::Error { message } => {
                Err(ErrorData::internal_error(message, None))
            }
            other => Err(unexpected(other)),
        }
    }

    /// Post a reply to a question this agent received.
    #[tool(description = "Post a reply to an ask. Returns the new reply id.")]
    async fn post_reply(
        &self,
        Parameters(args): Parameters<PostReplyArgs>,
    ) -> Result<String, ErrorData> {
        let req = Request::PostReply {
            from: self.client.agent_name.clone(),
            ask_id: args.ask_id,
            text: args.text,
        };
        match self.client.request(req).await.map_err(ipc_err)? {
            Response::ReplyAck { reply_id } => Ok(reply_id.to_string()),
            Response::Error { message } => {
                Err(ErrorData::internal_error(message, None))
            }
            other => Err(unexpected(other)),
        }
    }

    /// List agents currently registered with the broker.
    #[tool(description = "List all agents registered with the broker. Returns a JSON array of AgentDto.")]
    async fn list_agents(&self) -> Result<String, ErrorData> {
        match self.client.request(Request::ListAgents).await.map_err(ipc_err)? {
            Response::Agents { agents } => to_json_string::<Vec<AgentDto>>(&agents),
            Response::Error { message } => {
                Err(ErrorData::internal_error(message, None))
            }
            other => Err(unexpected(other)),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Shim {}

// ---------------------------------------------------------------------------
// Entry point.
// ---------------------------------------------------------------------------

pub async fn run(socket: PathBuf, agent_token: String) -> Result<()> {
    let client = Arc::new(BrokerClient::connect(&socket, &agent_token).await?);
    tracing::info!(agent = %client.agent_name, "shim authenticated");
    let shim = Shim::new(client);
    let server = shim
        .serve(rmcp::transport::stdio())
        .await
        .context("starting MCP server on stdio")?;
    server.waiting().await.context("MCP server loop")?;
    Ok(())
}
