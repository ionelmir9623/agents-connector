use crate::broker::handlers;
use crate::broker::store::Store;
use crate::ipc::{read_frame_async, write_frame_async, Request};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info};

pub struct BrokerCtx {
    pub store: Arc<Store>,
    pub reply_notifiers: Mutex<HashMap<i64, broadcast::Sender<()>>>,
    pub shutdown_tx: broadcast::Sender<()>,
}

impl BrokerCtx {
    pub fn new(store: Arc<Store>, shutdown_tx: broadcast::Sender<()>) -> Self {
        Self {
            store,
            reply_notifiers: Mutex::new(HashMap::new()),
            shutdown_tx,
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
}

/// Run the broker server, listening on `socket_path` and using `store` for persistence.
/// Returns when a Shutdown request is received.
pub async fn serve(store: Arc<Store>, socket_path: &Path) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    info!("broker listening on {}", socket_path.display());

    let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
    let ctx = Arc::new(BrokerCtx::new(store, shutdown_tx.clone()));

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
        let response = handlers::dispatch(request, &ctx).await;
        let bytes = serde_json::to_vec(&response)?;
        write_frame_async(&mut stream, &bytes).await?;
    }
}
