//! Live console for a single instance over a WebSocket: on connect, replays
//! a tail of `console.log`, then streams new lines as they're appended
//! (polling, same approach as `odin logs --follow`), while any text message
//! from the client is sent into the instance's console FIFO as a console
//! command (`odin exec`'s mechanism, `web::supervisor::Supervisor`).

use std::path::{Path as StdPath, PathBuf};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};

use crate::instance::Instance;
use crate::paths;
use crate::web::error::ApiError;
use crate::web::state::AppState;
use crate::web::supervisor::Supervisor;

const TAIL_LINES: usize = 200;
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub async fn console_ws(
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
    let supervisor = state.supervisor.clone();

    Ok(ws.on_upgrade(move |socket| handle_console_socket(socket, name, log_file, supervisor)))
}

async fn handle_console_socket(
    socket: WebSocket,
    name: String,
    log_file: PathBuf,
    supervisor: Supervisor,
) {
    let (mut sink, mut stream) = socket.split();

    let tail_log_file = log_file.clone();
    let (mut pos, tail) = tokio::task::spawn_blocking(move || {
        let tail = crate::commands::logs::read_tail(&tail_log_file, TAIL_LINES).unwrap_or_default();
        let pos = std::fs::metadata(&tail_log_file)
            .map(|m| m.len())
            .unwrap_or(0);
        (pos, tail)
    })
    .await
    .unwrap_or((0, String::new()));

    for line in tail.lines() {
        if sink.send(Message::text(line.to_string())).await.is_err() {
            return;
        }
    }

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            let Message::Text(text) = msg else { continue };
            let command = text.to_string();
            if let Err(error) = supervisor.send_command(&name, &command).await {
                // Transient right after an `odin serve` restart (the
                // reconciliation tick reopens a writer within a few
                // seconds) — log rather than drop silently, but don't tear
                // down the socket over it.
                tracing::warn!(instance = %name, %error, "console command not delivered");
            }
        }
    });

    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            let file = log_file.clone();
            let (new_pos, chunk) = tokio::task::spawn_blocking(move || read_new_bytes(&file, pos))
                .await
                .unwrap_or((pos, String::new()));
            pos = new_pos;
            if chunk.is_empty() {
                continue;
            }
            for line in chunk.lines() {
                if sink.send(Message::text(line.to_string())).await.is_err() {
                    return;
                }
            }
        }
    });

    tokio::select! {
        _ = &mut recv_task => send_task.abort(),
        _ = &mut send_task => recv_task.abort(),
    }
}

/// Reads whatever has been appended to `path` since byte offset `from`.
/// Restarts from the beginning if the file is now shorter than `from`
/// (rotated/truncated). Never errors — a transient read failure just yields
/// no new bytes this tick, and the next poll tries again.
///
/// Also used by `web::players`' console-log tailer — same poll-for-new-bytes
/// mechanism, just applied to player-connection parsing instead of a client
/// socket.
pub(crate) fn read_new_bytes(path: &StdPath, from: u64) -> (u64, String) {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = std::fs::File::open(path) else {
        return (from, String::new());
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(from);
    let start = if len < from { 0 } else { from };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return (len, String::new());
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return (len, String::new());
    }
    (len, String::from_utf8_lossy(&buf).to_string())
}
