use anyhow::Result;

use crate::activity::{ActivityKind, ActivityLog};
use crate::db::{self, Db};
use crate::valheim_update::{self, UpdateStatus};
use crate::web::state::AppState;

const LAST_NOTIFIED_BUILD_ID_CACHE_KEY: &str = "valheim_last_notified_build_id";

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            let tick_state = state.clone();
            match tokio::task::spawn_blocking(move || run_tick(&tick_state)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%error, "failed to check for Valheim server updates");
                }
                Err(error) => {
                    tracing::warn!(%error, "Valheim update monitor task panicked");
                }
            }
            tokio::time::sleep(valheim_update::CHECK_INTERVAL).await;
        }
    });
}

fn run_tick(state: &AppState) -> Result<()> {
    let status = valheim_update::check(&state.paths, &state.db)?;
    record_update_if_new(&state.db, &state.activity, &status)
}

fn record_update_if_new(db: &Db, activity: &ActivityLog, status: &UpdateStatus) -> Result<()> {
    let (Some(installed_build_id), Some(latest_build_id)) =
        (status.installed_build_id, status.latest_build_id)
    else {
        return Ok(());
    };
    if !status.update_available {
        return Ok(());
    }

    let latest_build_id_string = latest_build_id.to_string();
    if db::cache::get(db, LAST_NOTIFIED_BUILD_ID_CACHE_KEY)?
        .is_some_and(|entry| entry.value == latest_build_id_string)
    {
        return Ok(());
    }

    activity.record(
        ActivityKind::ServerUpdateAvailable {
            installed_build_id,
            latest_build_id,
        },
        None,
    );
    db::cache::set(
        db,
        LAST_NOTIFIED_BUILD_ID_CACHE_KEY,
        &latest_build_id_string,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::paths::Paths;

    fn temp_db() -> Arc<Db> {
        let dir = std::env::temp_dir().join(format!(
            "odin-update-monitor-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(
            Db::open(&Paths {
                data_dir: dir.clone(),
                config_dir: dir,
            })
            .unwrap(),
        )
    }

    #[test]
    fn repeated_checks_record_one_event_for_the_same_latest_build() {
        let db = temp_db();
        let activity = ActivityLog::load(db.clone());
        let status = UpdateStatus {
            installed_build_id: Some(100),
            latest_build_id: Some(200),
            update_available: true,
        };

        record_update_if_new(&db, &activity, &status).unwrap();
        record_update_if_new(&db, &activity, &status).unwrap();

        let (history, _rx) = activity.subscribe();
        assert_eq!(history.len(), 1);
    }
}
