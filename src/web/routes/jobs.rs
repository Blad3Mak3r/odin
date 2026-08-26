use std::convert::Infallible;

use async_stream::stream;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;

use crate::web::jobs::{JobEvent, JobSnapshot, JobSummary};
use crate::web::state::AppState;

pub async fn list_jobs(State(state): State<AppState>) -> Json<Vec<JobSummary>> {
    Json(state.jobs.list())
}

pub async fn get_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<JobSnapshot>, StatusCode> {
    state.jobs.get(&id).map(Json).ok_or(StatusCode::NOT_FOUND)
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireEvent<'a> {
    Log {
        line: &'a str,
    },
    Status {
        status: &'a crate::web::jobs::JobStatus,
    },
    Lagged {
        skipped: u64,
    },
}

pub async fn job_sse(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    let Some((log, status, rx)) = state.jobs.subscribe(&id) else {
        return Err(StatusCode::NOT_FOUND);
    };

    Ok(Sse::new(job_stream(log, status, rx)).keep_alive(KeepAlive::default()))
}

fn job_stream(
    log: Vec<String>,
    status: crate::web::jobs::JobStatus,
    mut rx: tokio::sync::broadcast::Receiver<JobEvent>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    stream! {
        for line in &log {
            yield Ok(json_event(&WireEvent::Log { line }));
        }
        yield Ok(json_event(&WireEvent::Status { status: &status }));

        loop {
            match rx.recv().await {
                Ok(JobEvent::Line(line)) => yield Ok(json_event(&WireEvent::Log { line: &line })),
                Ok(JobEvent::Status(status)) => {
                    yield Ok(json_event(&WireEvent::Status { status: &status }))
                }
                Err(RecvError::Lagged(skipped)) => {
                    yield Ok(json_event(&WireEvent::Lagged { skipped }))
                }
                Err(RecvError::Closed) => return,
            }
        }
    }
}

fn json_event(event: &WireEvent<'_>) -> Event {
    Event::default().data(serde_json::to_string(event).unwrap_or_default())
}
