//! Periodically removes expired dashboard history from SQLite and memory.

use std::time::Duration;

use chrono::Utc;

use crate::web::state::AppState;

const RETENTION: chrono::Duration = chrono::Duration::days(7);
/// Security events are worth keeping much longer than instance chatter.
const AUDIT_RETENTION: chrono::Duration = chrono::Duration::days(90);
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

    match crate::db::auth_events::purge_before(&state.db, Utc::now() - AUDIT_RETENTION) {
        Ok(deleted) if deleted > 0 => {
            tracing::info!(deleted, "purged expired audit events");
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(%error, "failed to purge expired audit events");
        }
    }

    if let Err(error) =
        crate::db::auth_throttle::purge_before(&state.db, Utc::now() - chrono::Duration::days(1))
    {
        tracing::warn!(%error, "failed to purge stale login counters");
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::activity::{ActivityEvent, ActivityKind};
    use crate::db::Db;
    use crate::game::GameId;
    use crate::paths::Paths;

    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!(
            "odin-retention-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        AppState::new(paths, db)
    }

    fn insert_event(state: &AppState, id: &str, age: chrono::Duration) {
        crate::db::activity::insert(
            &state.db,
            &ActivityEvent {
                id: id.to_string(),
                at: Utc::now() - age,
                game: GameId::Valheim,
                instance: None,
                instance_id: None,
                kind: ActivityKind::InstanceCreated,
            },
        )
        .unwrap();
    }

    #[test]
    fn expired_activity_is_purged_and_recent_activity_and_audit_events_are_kept() {
        let seed = state();
        insert_event(&seed, "old", RETENTION + chrono::Duration::hours(1));
        insert_event(&seed, "recent", chrono::Duration::hours(1));
        crate::db::auth_events::insert(&seed.db, "login", Some("admin"), None, None, None).unwrap();
        // Activity is loaded into memory when the state is built.
        let state = AppState::new(seed.paths.as_ref().clone(), seed.db.clone());

        run_once(&state);

        let (history, _rx) = state.activity.subscribe();
        let ids: Vec<_> = history.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, ["recent"]);
        assert_eq!(crate::db::activity::recent(&state.db, 10).unwrap().len(), 1);
        assert_eq!(
            crate::db::auth_events::recent(&state.db, 10).unwrap().len(),
            1
        );
    }
}
