//! Live log stream for a single instance over Server-Sent Events: replays a
//! tail of `console.log` on connect, then streams new lines as they're
//! appended — fed by the shared per-instance tailer in `web::log_tail`
//! rather than polling the file itself.

use std::convert::Infallible;

use async_stream::stream;
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::Stream;
use tokio::sync::broadcast;

use crate::game::GameId;
use crate::instance::Instance;
use crate::paths;
use crate::web::error::ApiError;
use crate::web::state::AppState;

const TAIL_LINES: usize = 200;

pub async fn logs_sse(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let load_name = name.clone();
    let instance =
        crate::web::error::run_blocking(move || Instance::load_existing(&paths, &db, &load_name))
            .await?;

    let log_file = paths::instance_logs_dir(&instance.dir).join("console.log");
    let receiver = state.log_tail.sender_for(&name).subscribe();

    let tail = tokio::task::spawn_blocking(move || {
        crate::commands::logs::read_tail(&log_file, TAIL_LINES).unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    Ok(Sse::new(log_stream(tail, receiver))
        .keep_alive(KeepAlive::default())
        .into_response())
}

pub async fn game_logs_sse(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> Result<Response, ApiError> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let load_name = name.clone();
    crate::web::error::run_blocking(move || {
        crate::game::instances::load(&paths, &db, game, &load_name)
    })
    .await?;

    let log_file = crate::paths::instance_logs_dir(&state.paths.game_instance_dir(game, &name))
        .join("console.log");
    let tail_file = log_file.clone();
    let tail = tokio::task::spawn_blocking(move || {
        crate::commands::logs::read_tail(&tail_file, TAIL_LINES).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    let position = std::fs::metadata(&log_file)
        .map(|metadata| metadata.len())
        .unwrap_or(0);

    Ok(Sse::new(file_log_stream(tail, log_file, position))
        .keep_alive(KeepAlive::default())
        .into_response())
}

fn file_log_stream(
    tail: String,
    log_file: std::path::PathBuf,
    mut position: u64,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        for line in tail.lines() {
            yield Ok(Event::default().data(line));
        }
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let file = log_file.clone();
            let (next_position, chunk) = tokio::task::spawn_blocking(move || {
                crate::log_poll::read_new_bytes(&file, position)
            }).await.unwrap_or((position, String::new()));
            position = next_position;
            for line in chunk.lines() {
                yield Ok(Event::default().data(line));
            }
        }
    }
}

fn log_stream(
    tail: String,
    mut receiver: broadcast::Receiver<String>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        for line in tail.lines() {
            yield Ok(Event::default().data(line));
        }

        loop {
            match receiver.recv().await {
                Ok(line) => yield Ok(Event::default().data(line)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    }
}
