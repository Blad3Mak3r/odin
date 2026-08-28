use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::activity::ActivityKind;
use crate::backup::{self, BackupEntry};
use crate::db::backup_schedules;
use crate::instance::Instance;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::jobs::JobKindDescr;
use crate::web::routes::mods::JobHandle;
use crate::web::state::AppState;

pub async fn list_backups(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Vec<BackupEntry>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let entries = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        backup::list(&db, &instance.state.name)
    })
    .await?;
    Ok(Json(entries))
}

// See the comment on `mods::add_mod`: spawning a job can't fail
// synchronously, so there's nothing for `ApiResult` to wrap.
pub async fn create_backup(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<JobHandle> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state.jobs.spawn(
        JobKindDescr::BackupCreate {
            instance: name.clone(),
        },
        move |logger| {
            logger.line(format!("backing up '{name}'"));
            let instance = Instance::load_existing(&paths, &db, &name)?;
            let result = backup::create(&instance, &db);
            if let Ok(path) = &result {
                logger.line(format!("done: {}", path.display()));
                let backup_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                activity.record(
                    ActivityKind::BackupCreated { backup_id },
                    Some(name.clone()),
                );
            }
            result.map(|_| ())
        },
    );
    Json(JobHandle { id })
}

pub async fn restore_backup(
    State(state): State<AppState>,
    Path((name, backup_id)): Path<(String, String)>,
) -> Json<JobHandle> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state.jobs.spawn(
        JobKindDescr::BackupRestore {
            instance: name.clone(),
            backup_id: backup_id.clone(),
        },
        move |logger| {
            logger.line(format!("restoring '{name}' from backup '{backup_id}'"));
            let instance = Instance::load_existing(&paths, &db, &name)?;
            let result = backup::restore(&instance, &db, &backup_id);
            if result.is_ok() {
                logger.line("done");
                activity.record(
                    ActivityKind::BackupRestored {
                        backup_id: backup_id.clone(),
                    },
                    Some(name.clone()),
                );
            }
            result
        },
    );
    Json(JobHandle { id })
}

pub async fn delete_backup(
    State(state): State<AppState>,
    Path((name, backup_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        backup::delete(&instance, &db, &backup_id)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct BackupScheduleView {
    pub interval_hours: u32,
    pub retain_count: u32,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
}

impl Default for BackupScheduleView {
    /// What an instance with no schedule configured yet gets back — a
    /// sensible starting point (daily, keep a week) with `enabled: false`,
    /// so turning scheduling on doesn't require also picking values first.
    fn default() -> Self {
        Self {
            interval_hours: 24,
            retain_count: 7,
            enabled: false,
            last_run_at: None,
        }
    }
}

pub async fn get_backup_schedule(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<BackupScheduleView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let view = run_blocking(move || {
        // Loaded only to 404 on an unknown instance, matching every other
        // per-instance route.
        Instance::load_existing(&paths, &db, &name)?;
        let schedule = backup_schedules::get(&db, &name)?;
        Ok(match schedule {
            Some(s) => BackupScheduleView {
                interval_hours: s.interval_hours,
                retain_count: s.retain_count,
                enabled: s.enabled,
                last_run_at: s.last_run_at,
            },
            None => BackupScheduleView::default(),
        })
    })
    .await?;
    Ok(Json(view))
}

#[derive(Deserialize)]
pub struct SetBackupScheduleRequest {
    pub interval_hours: u32,
    pub retain_count: u32,
    pub enabled: bool,
}

pub async fn set_backup_schedule(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<SetBackupScheduleRequest>,
) -> ApiResult<Json<BackupScheduleView>> {
    if req.interval_hours == 0 {
        return Err(BadRequest("interval_hours must be at least 1".to_string()).into());
    }
    if req.retain_count == 0 {
        return Err(BadRequest("retain_count must be at least 1".to_string()).into());
    }

    let paths = state.paths.clone();
    let db = state.db.clone();
    let view = run_blocking(move || {
        Instance::load_existing(&paths, &db, &name)?;
        backup_schedules::upsert(
            &db,
            &name,
            req.interval_hours,
            req.retain_count,
            req.enabled,
        )?;
        let last_run_at = backup_schedules::get(&db, &name)?.and_then(|s| s.last_run_at);
        Ok(BackupScheduleView {
            interval_hours: req.interval_hours,
            retain_count: req.retain_count,
            enabled: req.enabled,
            last_run_at,
        })
    })
    .await?;
    Ok(Json(view))
}
