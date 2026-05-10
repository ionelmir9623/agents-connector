use crate::broker::handlers;
use crate::broker::store::Store;
use crate::ipc::{read_frame_async, write_frame_async, Request};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::net::UnixListener;
use tracing::{error, info};

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

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _addr)) => {
                        let store = Arc::clone(&store);
                        let shutdown_tx = shutdown_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, store, shutdown_tx).await {
                                error!("connection error: {:#}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("accept error: {:#}", e);
                    }
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
    store: Arc<Store>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> Result<()> {
    loop {
        let frame = match read_frame_async(&mut stream).await {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let request: Request = serde_json::from_slice(&frame)?;
        let response = handlers::dispatch(request, &store, &shutdown_tx).await;
        let bytes = serde_json::to_vec(&response)?;
        write_frame_async(&mut stream, &bytes).await?;
    }
}
