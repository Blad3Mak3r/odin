//! Background loop that creates a backup for any instance with an enabled
//! schedule that's come due, then prunes old backups beyond the schedule's
//! retain count. Reuses the same job system and backup primitives the
//! manual "Create backup" button uses, so a scheduled backup looks
//! identical in the Jobs page to one an admin triggered by hand.

use std::time::Duration;

use chrono::Utc;

use crate::activity::{ActivityKind, ActivityLog};
use crate::backup;
use crate::db::Db;
use crate::db::backup_schedules::{self, BackupScheduleRow};
use crate::instance::Instance;
use crate::web::jobs::{JobKindDescr, JobLogger};
use crate::web::state::AppState;

const CHECK_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            let tick_state = state.clone();
            let _ = tokio::task::spawn_blocking(move || run_tick(&tick_state)).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

fn run_tick(state: &AppState) {
    let now = Utc::now();
    let due = match backup_schedules::due(&state.db, now) {
        Ok(due) => due,
        Err(e) => {
            tracing::warn!(error = %e, "failed to load due backup schedules");
            return;
        }
    };

    for schedule in due {
        // Mark the schedule as run *before* the backup job finishes, not
        // after — a backup can take a while, and this check only runs once
        // a minute, so marking it late would let a slow job get
        // re-triggered on the next tick while it's still in flight.
        if let Err(e) = backup_schedules::mark_run(&state.db, &schedule.instance_name, now) {
            tracing::warn!(
                instance = %schedule.instance_name,
                error = %e,
                "failed to mark backup schedule as run",
            );
            continue;
        }
        spawn_scheduled_backup(state, schedule);
    }
}

fn spawn_scheduled_backup(state: &AppState, schedule: BackupScheduleRow) {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let name = schedule.instance_name.clone();
    let retain_count = schedule.retain_count as usize;

    state.jobs.spawn(
        JobKindDescr::BackupCreate {
            instance: name.clone(),
        },
        move |logger| {
            logger.line(format!("scheduled backup of '{name}'"));
            let instance = Instance::load_existing(&paths, &db, &name)?;
            let path = backup::create(&instance, &db)?;
            let backup_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            logger.line(format!("done: {}", path.display()));
            activity.record(
                ActivityKind::BackupCreated { backup_id },
                Some(name.clone()),
            );

            prune_old_backups(&instance, &db, &activity, &name, retain_count, logger);
            Ok(())
        },
    );
}

fn prune_old_backups(
    instance: &Instance,
    db: &Db,
    activity: &ActivityLog,
    name: &str,
    retain_count: usize,
    logger: &JobLogger,
) {
    let backups = match backup::list(db, name) {
        Ok(backups) => backups,
        Err(e) => {
            logger.line(format!("could not list backups to prune: {e:#}"));
            return;
        }
    };

    for old in backups.into_iter().skip(retain_count) {
        match backup::delete(instance, db, &old.id) {
            Ok(()) => {
                logger.line(format!("pruned old backup '{}'", old.id));
                activity.record(
                    ActivityKind::BackupPruned { backup_id: old.id },
                    Some(name.to_string()),
                );
            }
            Err(e) => logger.line(format!("failed to prune backup '{}': {e:#}", old.id)),
        }
    }
}
