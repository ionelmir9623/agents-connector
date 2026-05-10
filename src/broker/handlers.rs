use crate::broker::server::BrokerCtx;
use crate::ipc::{Request, Response};
use std::sync::Arc;

pub async fn dispatch(req: Request, ctx: &Arc<BrokerCtx>) -> Response {
    match req {
        Request::Authenticate { agent_token } => match ctx.store.agent_by_token(&agent_token) {
            Ok(Some(agent)) => Response::AgentInfo { name: agent.name, cli_kind: agent.cli_kind },
            Ok(None) => Response::Error { message: "unknown agent token".into() },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::RegisterAgent { name, cli_kind } => match ctx.store.register_agent(&name, &cli_kind) {
            Ok(token) => Response::RegisterAck { agent_token: token },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::ListAgents => match ctx.store.list_agents() {
            Ok(agents) => Response::Agents {
                agents: agents.into_iter().map(|a| crate::ipc::AgentDto {
                    name: a.name,
                    cli_kind: a.cli_kind,
                }).collect(),
            },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::Tell { from, to, text, urgent: _ } => {
            let from_dto = from.clone();
            let to_dto = to.clone();
            let text_dto = text.clone();
            match ctx.store.tell(&from, to.as_deref(), &text) {
                Ok(message_id) => {
                    let dto = crate::ipc::MessageDto {
                        id: message_id,
                        from: from_dto,
                        to: to_dto,
                        text: text_dto,
                        ask_id: None,
                        in_reply_to: None,
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    let _ = ctx.message_stream.send(dto);
                    Response::TellAck { message_id }
                }
                Err(e) => Response::Error { message: format!("{:#}", e) },
            }
        }
        Request::ReadMessages { agent, since } => match ctx.store.read_messages_for(&agent, since) {
            Ok(msgs) => Response::Messages {
                messages: msgs.into_iter().map(message_to_dto).collect(),
            },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::Ask { from, to, text } => {
            let from_dto = from.clone();
            let to_dto = to.clone();
            let text_dto = text.clone();
            match ctx.store.ask(&from, &to, &text) {
                Ok(result) => {
                    let dto = crate::ipc::MessageDto {
                        id: result.message_id,
                        from: from_dto,
                        to: Some(to_dto),
                        text: text_dto,
                        ask_id: Some(result.ask_id),
                        in_reply_to: None,
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    let _ = ctx.message_stream.send(dto);
                    Response::AskAck { ask_id: result.ask_id }
                }
                Err(e) => Response::Error { message: format!("{:#}", e) },
            }
        }
        Request::PostReply { from, ask_id, text } => {
            let from_dto = from.clone();
            let text_dto = text.clone();
            match ctx.store.post_reply(&from, ask_id, &text) {
                Ok(result) => {
                    ctx.fire_reply(ask_id).await;
                    let dto = crate::ipc::MessageDto {
                        id: result.message_id,
                        from: from_dto,
                        to: Some(result.original_asker),
                        text: text_dto,
                        ask_id: None,
                        in_reply_to: Some(ask_id),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    };
                    let _ = ctx.message_stream.send(dto);
                    Response::ReplyAck { reply_id: result.reply_id }
                }
                Err(e) => Response::Error { message: format!("{:#}", e) },
            }
        }
        Request::CheckReplies { ask_id } => match ctx.store.replies_for_ask(ask_id) {
            Ok(replies) => Response::Replies {
                replies: replies.into_iter().map(reply_to_dto).collect(),
            },
            Err(e) => Response::Error { message: format!("{:#}", e) },
        },
        Request::WaitForReply { ask_id, timeout_ms } => {
            // Subscribe BEFORE checking, so a reply that arrives in between still wakes us.
            let notifier = ctx.notifier_for(ask_id).await;
            let mut rx = notifier.subscribe();
            // Cheap path: there might already be replies.
            if let Ok(replies) = ctx.store.replies_for_ask(ask_id) {
                if !replies.is_empty() {
                    return Response::Replies {
                        replies: replies.into_iter().map(reply_to_dto).collect(),
                    };
                }
            }
            // Wait for either the notifier or timeout.
            let timeout = std::time::Duration::from_millis(timeout_ms);
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(_) => match ctx.store.replies_for_ask(ask_id) {
                    Ok(replies) => Response::Replies {
                        replies: replies.into_iter().map(reply_to_dto).collect(),
                    },
                    Err(e) => Response::Error { message: format!("{:#}", e) },
                },
                Err(_) => Response::Replies { replies: vec![] }, // timeout = empty
            }
        }
        Request::Shutdown => {
            let _ = ctx.shutdown_tx.send(());
            Response::Ok
        }
        Request::SubscribeStream => {
            // Intercepted by handle_connection before dispatch; this arm is a defensive fallback.
            Response::Error { message: "subscribe_stream must be handled at connection level".into() }
        }
    }
}

fn message_to_dto(m: crate::broker::store::Message) -> crate::ipc::MessageDto {
    crate::ipc::MessageDto {
        id: m.id,
        from: m.from_name,
        to: m.to_name,
        text: m.text,
        ask_id: m.ask_id,
        in_reply_to: m.in_reply_to,
        created_at: m.created_at.to_rfc3339(),
    }
}

fn reply_to_dto(r: crate::broker::store::Reply) -> crate::ipc::ReplyDto {
    crate::ipc::ReplyDto {
        id: r.id,
        ask_id: r.ask_id,
        from: r.from_name,
        text: r.text,
        created_at: r.created_at.to_rfc3339(),
    }
}
