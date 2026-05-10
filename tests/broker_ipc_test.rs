use agents_connector::broker::store::Store;
use agents_connector::broker::server;
use agents_connector::ipc::{read_frame_async, write_frame_async, Request, Response};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::UnixStream;

async fn spawn_test_broker() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone, None).await.unwrap();
    });
    // Allow the listener to bind. A short retry loop is more robust than a sleep.
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    (tmp, sock)
}

#[tokio::test]
async fn authenticate_with_valid_token_returns_ok() {
    let (_tmp, sock) = spawn_test_broker().await;
    // Pre-register an agent directly via the store so we have a token.
    let store = Store::open(&_tmp.path().join("test.sqlite")).unwrap();
    let token = store.register_agent("alice", "claude", None).unwrap();

    let mut stream = UnixStream::connect(&sock).await.unwrap();
    let req = Request::Authenticate { agent_token: token };
    let bytes = serde_json::to_vec(&req).unwrap();
    write_frame_async(&mut stream, &bytes).await.unwrap();

    let frame = read_frame_async(&mut stream).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::AgentInfo { name, cli_kind } => {
            assert_eq!(name, "alice");
            assert_eq!(cli_kind, "claude");
        }
        other => panic!("unexpected response: {:?}", other),
    }
}

#[tokio::test]
async fn register_agent_returns_token_and_list_includes_it() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut stream = UnixStream::connect(&sock).await.unwrap();
    let req = Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into(), workdir: None };
    write_frame_async(&mut stream, &serde_json::to_vec(&req).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut stream).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    let token = match resp {
        Response::RegisterAck { agent_token } => agent_token,
        other => panic!("unexpected: {:?}", other),
    };
    assert!(!token.is_empty());

    let req = Request::ListAgents;
    write_frame_async(&mut stream, &serde_json::to_vec(&req).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut stream).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::Agents { agents } => {
            assert_eq!(agents.len(), 1);
            assert_eq!(agents[0].name, "alice");
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn tell_and_read_messages_round_trip() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into(), workdir: None }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "bob".into(), cli_kind: "claude".into(), workdir: None }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    write_frame_async(&mut s, &serde_json::to_vec(&Request::Tell {
        from: "alice".into(), to: Some("bob".into()), text: "hello".into(), urgent: false,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    let msg_id = match resp {
        Response::TellAck { message_id } => message_id,
        other => panic!("unexpected: {:?}", other),
    };
    assert!(msg_id > 0);

    write_frame_async(&mut s, &serde_json::to_vec(&Request::ReadMessages {
        agent: "bob".into(), since: 0,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::Messages { messages } => {
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].text, "hello");
            assert_eq!(messages[0].from, "alice");
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn ask_reply_check_round_trip() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into(), workdir: None }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "bob".into(), cli_kind: "claude".into(), workdir: None }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    write_frame_async(&mut s, &serde_json::to_vec(&Request::Ask {
        from: "alice".into(), to: "bob".into(), text: "still there?".into(),
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let ask_id = match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::AskAck { ask_id } => ask_id,
        other => panic!("unexpected: {:?}", other),
    };

    write_frame_async(&mut s, &serde_json::to_vec(&Request::PostReply {
        from: "bob".into(), ask_id, text: "yes".into(),
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::ReplyAck { .. } => {}
        other => panic!("unexpected: {:?}", other),
    }

    write_frame_async(&mut s, &serde_json::to_vec(&Request::CheckReplies { ask_id }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::Replies { replies } => {
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0].text, "yes");
            assert_eq!(replies[0].from, "bob");
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn wait_for_reply_blocks_then_returns() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "alice".into(), cli_kind: "claude".into(), workdir: None }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent { name: "bob".into(), cli_kind: "claude".into(), workdir: None }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    write_frame_async(&mut s, &serde_json::to_vec(&Request::Ask {
        from: "alice".into(), to: "bob".into(), text: "ready?".into(),
    }).unwrap()).await.unwrap();
    let ask_id = match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::AskAck { ask_id } => ask_id,
        other => panic!("unexpected: {:?}", other),
    };

    // Spawn a writer that posts a reply after 200ms.
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let mut s2 = UnixStream::connect(&sock_clone).await.unwrap();
        write_frame_async(&mut s2, &serde_json::to_vec(&Request::PostReply {
            from: "bob".into(), ask_id, text: "go".into(),
        }).unwrap()).await.unwrap();
        let _ = read_frame_async(&mut s2).await.unwrap();
    });

    write_frame_async(&mut s, &serde_json::to_vec(&Request::WaitForReply { ask_id, timeout_ms: 2000 }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::Replies { replies } => {
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0].text, "go");
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn get_agent_returns_full_details() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: Some("/tmp/x".into()),
    }).unwrap()).await.unwrap();
    let token = match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::RegisterAck { agent_token } => agent_token,
        other => panic!("unexpected: {:?}", other),
    };

    write_frame_async(&mut s, &serde_json::to_vec(&Request::GetAgent { name: "alice".into() }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::AgentDetails { name, cli_kind, token: t, workdir } => {
            assert_eq!(name, "alice");
            assert_eq!(cli_kind, "claude");
            assert_eq!(t, token);
            assert_eq!(workdir.as_deref(), Some("/tmp/x"));
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn remove_agent_soft_deletes_via_ipc() {
    let (_tmp, sock) = spawn_test_broker().await;

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: None,
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    write_frame_async(&mut s, &serde_json::to_vec(&Request::RemoveAgent { name: "alice".into() }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::RemoveAck { freed_token } => assert!(!freed_token.is_empty()),
        other => panic!("unexpected: {:?}", other),
    }

    // Subsequent GetAgent fails.
    write_frame_async(&mut s, &serde_json::to_vec(&Request::GetAgent { name: "alice".into() }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::Error { message } => assert!(message.contains("not found")),
        other => panic!("unexpected: {:?}", other),
    }
}
