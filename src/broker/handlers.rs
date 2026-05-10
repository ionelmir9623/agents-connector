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
        _ => Response::Error { message: "not yet implemented".into() },
    }
}
