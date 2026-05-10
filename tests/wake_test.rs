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
        name: "alice".into(), cli_kind: "claude".into(), workdir: None,
    }).unwrap()).await.unwrap();
    let _ = read_frame_async(&mut s).await.unwrap();
    write_frame_async(&mut s, &serde_json::to_vec(&Request::RegisterAgent {
        name: "bob".into(), cli_kind: "claude".into(), workdir: None,
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
