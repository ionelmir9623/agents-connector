//! Verifies the urgent flag wake path:
//! - Plumbs through Tell when urgent=true and to is set.
//!
//! These tests don't actually invoke tmux — they set the disable env var so the
//! wake helper short-circuits. We're verifying the dispatch logic, not tmux's
//! behavior (that's only verifiable in a real terminal).

use agents_connector::broker::{server, store::Store};
use agents_connector::ipc::{read_frame_async, write_frame_async, Request, Response};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::net::UnixStream;

#[tokio::test]
async fn urgent_tell_completes_with_wake_disabled() {
    std::env::set_var("AGENTS_CONNECTOR_DISABLE_WAKE", "1");

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone, Some("test-session".into())).await.unwrap();
    });
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "bob".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    // Send an URGENT tell; with wake disabled this should still ack normally.
    write_frame_async(&mut s, &serde_json::to_vec(&Request::Tell {
        from: "alice".into(),
        to: Some("bob".into()),
        text: "WAKE UP".into(),
        urgent: true,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::TellAck { message_id } => assert!(message_id > 0),
        other => panic!("unexpected: {:?}", other),
    }

    std::env::remove_var("AGENTS_CONNECTOR_DISABLE_WAKE");
}

#[tokio::test]
async fn urgent_ask_completes_with_wake_disabled() {
    std::env::set_var("AGENTS_CONNECTOR_DISABLE_WAKE", "1");

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone, Some("test-session".into())).await.unwrap();
    });
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "bob".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    // Send an URGENT ask; with wake disabled this should still ack normally.
    write_frame_async(&mut s, &serde_json::to_vec(&Request::Ask {
        from: "alice".into(),
        to: "bob".into(),
        text: "are you ready?".into(),
        urgent: true,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::AskAck { ask_id } => assert!(ask_id > 0),
        other => panic!("unexpected: {:?}", other),
    }

    std::env::remove_var("AGENTS_CONNECTOR_DISABLE_WAKE");
}

#[tokio::test]
async fn wake_skips_busy_agent() {
    std::env::set_var("AGENTS_CONNECTOR_DISABLE_WAKE", "1");

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone, Some("test-session".into())).await.unwrap();
    });
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let token = match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::RegisterAck { agent_token } => agent_token,
        other => panic!("unexpected: {:?}", other),
    };
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "bob".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    // Set alice's state to busy via SetAgentState IPC.
    write_frame_async(&mut s, &serde_json::to_vec(&Request::SetAgentState {
        agent_token: token,
        state: "busy".into(),
    }).unwrap()).await.unwrap();
    match serde_json::from_slice::<Response>(&read_frame_async(&mut s).await.unwrap()).unwrap() {
        Response::Ok => {}
        other => panic!("unexpected: {:?}", other),
    }

    // Send an urgent Tell to alice (who is busy). Should complete without error.
    write_frame_async(&mut s, &serde_json::to_vec(&Request::Tell {
        from: "bob".into(),
        to: Some("alice".into()),
        text: "hello while busy".into(),
        urgent: true,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    let resp: Response = serde_json::from_slice(&frame).unwrap();
    match resp {
        Response::TellAck { message_id } => assert!(message_id > 0),
        other => panic!("unexpected: {:?}", other),
    }

    // Verify alice's state is still busy (the message is queued, not lost).
    let store2 = Store::open(&db).unwrap();
    let agent = store2.agent_by_name("alice").unwrap().unwrap();
    assert_eq!(agent.state, "busy");

    std::env::remove_var("AGENTS_CONNECTOR_DISABLE_WAKE");
}

#[tokio::test]
async fn stale_busy_state_auto_resets_on_wake() {
    // If an agent's state has been `busy` for > 10 minutes, we treat it as wedged
    // (missed `stop` hook due to crash) and proceed with the wake. The state is
    // also reset to `idle` so subsequent reads are accurate.
    std::env::set_var("AGENTS_CONNECTOR_DISABLE_WAKE", "1");

    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("test.sqlite");
    let sock = tmp.path().join("broker.sock");
    let store = Arc::new(Store::open(&db).unwrap());
    let sock_clone = sock.clone();
    tokio::spawn(async move {
        server::serve(store, &sock_clone, Some("test-session".into())).await.unwrap();
    });
    for _ in 0..50 {
        if sock.exists() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let mut s = UnixStream::connect(&sock).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "alice".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "bob".into(), cli_kind: "claude".into(), workdir: None, extra_args: vec![],
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();

    // Backdate alice's state to 15 minutes ago by direct SQL UPDATE — simulates
    // a wedged-busy state from a hook crash 15 minutes ago.
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let stale_ts = (chrono::Utc::now() - chrono::Duration::minutes(15)).to_rfc3339();
        conn.execute(
            "UPDATE agents SET state = 'busy', state_updated_at = ?1 WHERE name = 'alice'",
            rusqlite::params![stale_ts],
        ).unwrap();
    }

    // Confirm alice is wedged-busy.
    let store_check = Store::open(&db).unwrap();
    assert_eq!(store_check.agent_by_name("alice").unwrap().unwrap().state, "busy");

    // Send an urgent Tell. Wake decision should treat alice as idle (stale) and proceed.
    write_frame_async(&mut s, &serde_json::to_vec(&Request::Tell {
        from: "bob".into(),
        to: Some("alice".into()),
        text: "ping after long silence".into(),
        urgent: true,
    }).unwrap()).await.unwrap();
    let frame = read_frame_async(&mut s).await.unwrap();
    match serde_json::from_slice::<Response>(&frame).unwrap() {
        Response::TellAck { message_id } => assert!(message_id > 0),
        other => panic!("unexpected: {:?}", other),
    }

    // Now alice's state should have been auto-reset to idle.
    let store_after = Store::open(&db).unwrap();
    let alice_after = store_after.agent_by_name("alice").unwrap().unwrap();
    assert_eq!(alice_after.state, "idle", "stale busy should be auto-reset");

    std::env::remove_var("AGENTS_CONNECTOR_DISABLE_WAKE");
}
