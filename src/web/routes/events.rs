//! The dashboard's single global live feed: on connect, replays recent
//! activity history, then streams new activity events and periodic
//! host/instance resource ticks as they happen — the one WebSocket the
//! frontend keeps open for the lifetime of the app, instead of polling
//! `/instances` and `/system/resources` on a timer.

use std::convert::Infallible;

use async_stream::stream;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;

use crate::activity::ActivityEvent;
use crate::game::GameId;
use crate::web::runtime::{GameInstanceTransitions, InstanceTransitions, ResourcesTick};
use crate::web::state::AppState;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireEvent<'a> {
    Activity {
        event: &'a ActivityEvent,
    },
    Resources {
        tick: &'a ResourcesTick,
    },
    Transitions {
        transitions: &'a InstanceTransitions,
    },
    GameTransitions {
        transitions: &'a GameInstanceTransitions,
    },
    Lagged {
        skipped: u64,
    },
}

pub async fn events_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (history, activity_rx) = state.activity.subscribe();
    let ticks_rx = state.runtime.subscribe_ticks();
    let (game_transitions, transitions_rx) = state.runtime.subscribe_game_transitions();

    let activity_stream = stream! {
        for event in &history {
            yield Ok(json_event(&WireEvent::Activity { event }));
        }

        let mut activity_rx = activity_rx;
        loop {
            match activity_rx.recv().await {
                Ok(event) => yield Ok(json_event(&WireEvent::Activity { event: &event })),
                Err(RecvError::Lagged(skipped)) => yield Ok(json_event(&WireEvent::Lagged { skipped })),
                Err(RecvError::Closed) => return,
            }
        }
    };

    let ticks_stream = stream! {
        let mut ticks_rx = ticks_rx;
        loop {
            match ticks_rx.recv().await {
                Ok(tick) => yield Ok(json_event(&WireEvent::Resources { tick: &tick })),
                Err(RecvError::Lagged(skipped)) => yield Ok(json_event(&WireEvent::Lagged { skipped })),
                Err(RecvError::Closed) => return,
            }
        }
    };

    let transitions_stream = stream! {
        let transitions = valheim_transitions(&game_transitions);
        yield Ok(json_event(&WireEvent::Transitions { transitions: &transitions }));
        yield Ok(json_event(&WireEvent::GameTransitions { transitions: &game_transitions }));

        let mut transitions_rx = transitions_rx;
        loop {
            match transitions_rx.recv().await {
                Ok(game_transitions) => {
                    let transitions = valheim_transitions(&game_transitions);
                    yield Ok(json_event(&WireEvent::Transitions { transitions: &transitions }));
                    yield Ok(json_event(&WireEvent::GameTransitions { transitions: &game_transitions }));
                }
                Err(RecvError::Lagged(skipped)) => yield Ok(json_event(&WireEvent::Lagged { skipped })),
                Err(RecvError::Closed) => return,
            }
        }
    };

    Sse::new(futures_util::stream::select(
        futures_util::stream::select(activity_stream, ticks_stream),
        transitions_stream,
    ))
    .keep_alive(KeepAlive::default())
}

fn valheim_transitions(transitions: &GameInstanceTransitions) -> InstanceTransitions {
    transitions
        .iter()
        .filter(|transition| transition.game == GameId::Valheim)
        .map(|transition| (transition.name.clone(), transition.transition))
        .collect()
}

fn json_event(event: &WireEvent<'_>) -> Event {
    Event::default().data(serde_json::to_string(event).unwrap_or_default())
}
