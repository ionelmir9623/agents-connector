use crate::broker::handlers;
use crate::broker::store::Store;
use crate::ipc::{read_frame_async, write_frame_async, MessageDto, Request, Response};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info};

const WAKE_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);

pub struct BrokerCtx {
    pub store: Arc<Store>,
    pub reply_notifiers: Mutex<HashMap<i64, broadcast::Sender<()>>>,
    pub shutdown_tx: broadcast::Sender<()>,
    pub message_stream: broadcast::Sender<MessageDto>,
    pub session: Option<String>,
    pub last_wake: Mutex<HashMap<String, std::time::Instant>>,
}

impl BrokerCtx {
    pub fn new(store: Arc<Store>, shutdown_tx: broadcast::Sender<()>, session: Option<String>) -> Self {
        let (msg_tx, _) = broadcast::channel::<MessageDto>(256);
        Self {
            store,
            reply_notifiers: Mutex::new(HashMap::new()),
            shutdown_tx,
            message_stream: msg_tx,
            session,
            last_wake: Mutex::new(HashMap::new()),
        }
    }

    pub async fn notifier_for(&self, ask_id: i64) -> broadcast::Sender<()> {
        let mut map = self.reply_notifiers.lock().await;
        map.entry(ask_id)
            .or_insert_with(|| broadcast::channel::<()>(1).0)
            .clone()
    }

    pub async fn fire_reply(&self, ask_id: i64) {
        let map = self.reply_notifiers.lock().await;
        if let Some(tx) = map.get(&ask_id) {
            let _ = tx.send(());
        }
    }

    /// Returns true if the per-agent 5-second wake cooldown has elapsed.
    /// Updates the cooldown timestamp on a true return.
    pub async fn should_wake_cooldown(&self, agent: &str) -> bool {
        let mut map = self.last_wake.lock().await;
        let now = std::time::Instant::now();
        match map.get(agent) {
            Some(prev) if now.duration_since(*prev) < WAKE_COOLDOWN => false,
            _ => {
                map.insert(agent.to_string(), now);
                true
            }
        }
    }
}

/// Run the broker server, listening on `socket_path` and using `store` for persistence.
/// Returns when a Shutdown request is received.
pub async fn serve(store: Arc<Store>, socket_path: &Path, session: Option<String>) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    info!("broker listening on {}", socket_path.display());

    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
    let ctx = Arc::new(BrokerCtx::new(store, shutdown_tx.clone(), session));

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let ctx = Arc::clone(&ctx);
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, ctx).await {
                                error!("connection error: {:#}", e);
                            }
                        });
                    }
                    Err(e) => error!("accept error: {:#}", e),
                }
            }
            _ = shutdown_rx.recv() => {
                info!("broker shutting down");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(socket_path);
    Ok(())
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    ctx: Arc<BrokerCtx>,
) -> Result<()> {
    loop {
        let frame = match read_frame_async(&mut stream).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let request: Request = serde_json::from_slice(&frame)?;
        if matches!(request, Request::SubscribeStream) {
            return run_stream(stream, ctx).await;
        }
        let response = handlers::dispatch(request, &ctx).await;
        let bytes = serde_json::to_vec(&response)?;
        write_frame_async(&mut stream, &bytes).await?;
    }
}

async fn run_stream(mut stream: tokio::net::UnixStream, ctx: Arc<BrokerCtx>) -> Result<()> {
    let mut rx = ctx.message_stream.subscribe();
    // Send an Ok ack once subscribed so client knows the stream is live.
    write_frame_async(&mut stream, &serde_json::to_vec(&Response::Ok)?).await?;
    loop {
        match rx.recv().await {
            Ok(dto) => {
                let frame = serde_json::to_vec(&Response::StreamEvent { message: dto })?;
                if write_frame_async(&mut stream, &frame).await.is_err() {
                    return Ok(()); // client disconnected
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return Ok(()),
        }
    }
}
