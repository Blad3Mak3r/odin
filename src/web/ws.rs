//! Live log stream for a single instance over a WebSocket: replays a tail
//! of `console.log` on connect, then streams new lines as they're appended
//! — fed by the shared per-instance tailer in `web::log_tail` rather than
//! polling the file itself.

use std::path::PathBuf;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;

use crate::instance::Instance;
use crate::paths;
use crate::web::error::ApiError;
use crate::web::state::AppState;

const TAIL_LINES: usize = 200;

pub async fn logs_ws(
    State(state): State<AppState>,
    Path(name): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let load_name = name.clone();
    let instance =
        crate::web::error::run_blocking(move || Instance::load_existing(&paths, &db, &load_name))
            .await?;

    let log_file = paths::instance_logs_dir(&instance.dir).join("console.log");
    let receiver = state.log_tail.sender_for(&name).subscribe();

    Ok(ws.on_upgrade(move |socket| handle_log_socket(socket, log_file, receiver)))
}

async fn handle_log_socket(
    socket: WebSocket,
    log_file: PathBuf,
    mut receiver: broadcast::Receiver<String>,
) {
    let (mut sink, mut stream) = socket.split();

    let tail = tokio::task::spawn_blocking(move || {
        crate::commands::logs::read_tail(&log_file, TAIL_LINES).unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    for line in tail.lines() {
        if sink.send(Message::text(line.to_string())).await.is_err() {
            return;
        }
    }

    // This stream is read-only, so there's nothing to act on from the
    // client — but draining it lets the close handshake work and lets us
    // notice a client disconnect promptly.
    let mut recv_task = tokio::spawn(async move { while stream.next().await.is_some() {} });

    let mut send_task = tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(line) => {
                    if sink.send(Message::text(line)).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    tokio::select! {
        _ = &mut recv_task => send_task.abort(),
        _ = &mut send_task => recv_task.abort(),
    }
}
