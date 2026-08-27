use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::activity::ActivityKind;
use crate::backup::{self, BackupEntry};
use crate::instance::Instance;
use crate::web::error::{ApiResult, run_blocking};
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
