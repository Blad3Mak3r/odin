use axum::Json;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::Serialize;

use crate::web::jobs::{JobEvent, JobSnapshot};
use crate::web::state::AppState;

pub async fn list_jobs(State(state): State<AppState>) -> Json<Vec<JobSnapshot>> {
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
}

pub async fn job_ws(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let Some((log, status, mut rx)) = state.jobs.subscribe(&id) else {
        return Err(StatusCode::NOT_FOUND);
    };

    Ok(ws.on_upgrade(move |mut socket| async move {
        for line in &log {
            if send_json(&mut socket, &WireEvent::Log { line })
                .await
                .is_err()
            {
                return;
            }
        }
        if send_json(&mut socket, &WireEvent::Status { status: &status })
            .await
            .is_err()
        {
            return;
        }

        while let Ok(event) = rx.recv().await {
            let result = match &event {
                JobEvent::Line(line) => send_json(&mut socket, &WireEvent::Log { line }).await,
                JobEvent::Status(status) => {
                    send_json(&mut socket, &WireEvent::Status { status }).await
                }
            };
            if result.is_err() {
                return;
            }
        }
    }))
}

async fn send_json(
    socket: &mut axum::extract::ws::WebSocket,
    event: &WireEvent<'_>,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(event).unwrap_or_default();
    socket.send(Message::text(text)).await
}
