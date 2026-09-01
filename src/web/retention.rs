//! Periodically removes expired dashboard history from SQLite and memory.

use std::time::Duration;

use chrono::Utc;

use crate::web::state::AppState;

const RETENTION: chrono::Duration = chrono::Duration::days(7);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn run_once(state: &AppState) {
    let before = Utc::now() - RETENTION;

    match state.activity.purge_before(before) {
        Ok(deleted) if deleted > 0 => {
            tracing::info!(deleted, "purged expired activity events");
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "failed to purge expired activity events");
        }
    }

    match state.jobs.purge_before(before) {
        Ok(deleted) if deleted > 0 => {
            tracing::info!(deleted, "purged expired jobs");
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "failed to purge expired jobs");
        }
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(CHECK_INTERVAL).await;
            let tick_state = state.clone();
            if let Err(error) = tokio::task::spawn_blocking(move || run_once(&tick_state)).await {
                tracing::warn!(%error, "history retention task panicked");
            }
        }
    });
}
