use agents_connector::broker::store::Store;
use agents_connector::broker::server;
use agents_connector::ipc::{read_frame_async, write_frame_async, Request, Response};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::UnixStream;

async fn spawn_broker() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone, None).await.unwrap();
    });
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    (tmp, sock)
}

#[tokio::test]
async fn alice_asks_bob_who_replies_and_alice_sees_reply() {
    let (_tmp, sock) = spawn_broker().await;

    // Connect as alice.
    let mut alice = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut alice).await.unwrap();

    // Connect as bob.
    let mut bob = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut bob, &serde_json::to_vec(&Request::RegisterAgent {
        name: "bob".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut bob).await.unwrap();

    // alice asks bob.
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::Ask {
        from: "alice".into(), to: "bob".into(), text: "are you ready?".into(), urgent: false,
    }).unwrap()).await.unwrap();
    let ask_id = match serde_json::from_slice::<Response>(&read_frame_async(&mut alice).await.unwrap()).unwrap() {
        Response::AskAck { ask_id } => ask_id,
        other => panic!("unexpected: {:?}", other),
    };

    // bob reads messages — sees the ask.
    write_frame_async(&mut bob, &serde_json::to_vec(&Request::ReadMessages {
        agent: "bob".into(), since: 0,
    }).unwrap()).await.unwrap();
    let msgs = match serde_json::from_slice::<Response>(&read_frame_async(&mut bob).await.unwrap()).unwrap() {
        Response::Messages { messages } => messages,
        other => panic!("unexpected: {:?}", other),
    };
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].text, "are you ready?");
    assert_eq!(msgs[0].ask_id, Some(ask_id));

    // bob replies.
    write_frame_async(&mut bob, &serde_json::to_vec(&Request::PostReply {
        from: "bob".into(), ask_id, text: "yes".into(),
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut bob).await.unwrap();

    // alice waits for reply (should return immediately).
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::WaitForReply {
        ask_id, timeout_ms: 1000,
    }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut alice).await.unwrap()).unwrap() {
        Response::Replies { replies } => {
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0].text, "yes");
            assert_eq!(replies[0].from, "bob");
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test]
async fn broadcast_visible_to_all_other_agents() {
    let (_tmp, sock) = spawn_broker().await;
    for n in &["alice", "bob", "carol"] {
        let mut s = UnixStream::connect(&sock).await.unwrap();
        write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
            name: n.to_string(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
        }).unwrap()).await.unwrap();
        let _ = read_frame_async(&mut s).await.unwrap();
    }

    let mut alice = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::Tell {
        from: "alice".into(), to: None, text: "hi all".into(), urgent: false,
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut alice).await.unwrap();

    for n in &["bob", "carol"] {
        let mut s = UnixStream::connect(&sock).await.unwrap();
        write_frame_async(&mut s, &serde_json::to_vec(&Request::ReadMessages {
            agent: n.to_string(), since: 0,
        }).unwrap()).await.unwrap();
        let msgs = match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
            Response::Messages { messages } => messages,
            other => panic!("unexpected: {:?}", other),
        };
        assert_eq!(msgs.len(), 1, "{} should see one broadcast", n);
        assert_eq!(msgs[0].text, "hi all");
    }

    // alice does NOT see her own broadcast.
    let mut alice = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut alice, &serde_json::to_vec(&Request::ReadMessages {
        agent: "alice".into(), since: 0,
    }).unwrap()).await.unwrap();
    let msgs = match serde_json::from_slice::<Response>(&read_frame_async(&mut alice).await.unwrap()).unwrap() {
        Response::Messages { messages } => messages,
        other => panic!("unexpected: {:?}", other),
    };
    assert_eq!(msgs.len(), 0);
}
