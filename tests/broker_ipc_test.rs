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
        server::serve(store, &sock_clone).await.unwrap();
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
    let token = store.register_agent("alice", "claude").unwrap();

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
