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
