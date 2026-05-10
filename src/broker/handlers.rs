use crate::broker::store::Store;
use crate::ipc::{Request, Response};
use std::sync::Arc;

pub async fn dispatch(
    req: Request,
    store: &Arc<Store>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) -> Response {
    match req {
        Request::Authenticate { agent_token } => match store.agent_by_token(&agent_token) {
            Ok(Some(agent)) => Response::AgentInfo { name: agent.name, cli_kind: agent.cli_kind },
            Ok(None) => Response::Error { message: "unknown agent token".into() },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::Shutdown => {
            let _ = shutdown_tx.send(());
            Response::Ok
        }
        Request::RegisterAgent { name, cli_kind } => match store.register_agent(&name, &cli_kind) {
            Ok(token) => Response::RegisterAck { agent_token: token },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::ListAgents => match store.list_agents() {
            Ok(agents) => Response::Agents {
                agents: agents.into_iter().map(|a| crate::ipc::AgentDto {
                    name: a.name,
                    cli_kind: a.cli_kind,
                }).collect(),
            },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::Tell { from, to, text, urgent: _ } => {
            // urgent not handled in v1 (Phase 1) — wake mechanism is Phase 3.
            match store.tell(&from, to.as_deref(), &text) {
                Ok(id) => Response::TellAck { message_id: id },
                Err(e) => Response::Error { message: format!("{:#}", e) },
            }
        }
        Request::ReadMessages { agent, since } => match store.read_messages_for(&agent, since) {
            Ok(msgs) => Response::Messages {
                messages: msgs.into_iter().map(|m| crate::ipc::MessageDto {
                    id: m.id,
                    from: m.from_name,
                    to: m.to_name,
                    text: m.text,
                    ask_id: m.ask_id,
                    in_reply_to: m.in_reply_to,
                    created_at: m.created_at.to_rfc3339(),
                }).collect(),
            },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        _ => Response::Error { message: "not yet implemented".into() },
    }
}
