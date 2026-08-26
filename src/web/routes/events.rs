//! The dashboard's single global live feed: on connect, replays recent
//! activity history, then streams new activity events and periodic
//! host/instance resource ticks as they happen — the one WebSocket the
//! frontend keeps open for the lifetime of the app, instead of polling
//! `/instances` and `/system/resources` on a timer.

use axum::extract::State;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::response::Response;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;

use crate::activity::ActivityEvent;
use crate::web::runtime::ResourcesTick;
use crate::web::state::AppState;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireEvent<'a> {
    Activity { event: &'a ActivityEvent },
    Resources { tick: &'a ResourcesTick },
    Lagged { skipped: u64 },
}

pub async fn events_ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    let (history, mut activity_rx) = state.activity.subscribe();
    let mut ticks_rx = state.runtime.subscribe_ticks();

    ws.on_upgrade(move |mut socket| async move {
        for event in &history {
            if send_json(&mut socket, &WireEvent::Activity { event })
                .await
                .is_err()
            {
                return;
            }
        }

        loop {
            tokio::select! {
                result = activity_rx.recv() => {
                    match result {
                        Ok(event) => {
                            if send_json(&mut socket, &WireEvent::Activity { event: &event }).await.is_err() {
                                return;
                            }
                        }
                        Err(RecvError::Lagged(skipped)) => {
                            if send_json(&mut socket, &WireEvent::Lagged { skipped }).await.is_err() {
                                return;
                            }
                        }
                        Err(RecvError::Closed) => return,
                    }
                }
                result = ticks_rx.recv() => {
                    match result {
                        Ok(tick) => {
                            if send_json(&mut socket, &WireEvent::Resources { tick: &tick }).await.is_err() {
                                return;
                            }
                        }
                        Err(RecvError::Lagged(skipped)) => {
                            if send_json(&mut socket, &WireEvent::Lagged { skipped }).await.is_err() {
                                return;
                            }
                        }
                        Err(RecvError::Closed) => return,
                    }
                }
            }
        }
    })
}

async fn send_json(
    socket: &mut axum::extract::ws::WebSocket,
    event: &WireEvent<'_>,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(event).unwrap_or_default();
    socket.send(Message::text(text)).await
}
