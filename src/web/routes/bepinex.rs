use std::cmp::Ordering;

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;

use crate::activity::ActivityKind;
use crate::instance::{Instance, InstanceError};
use crate::mods::bepinex::{self, UpdateOutcome};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::jobs::JobKindDescr;
use crate::web::routes::mods::JobHandle;
use crate::web::runtime::InstanceTransition;
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct BepInExStatus {
    installed: bool,
    installed_version: Option<String>,
    latest_version: Option<String>,
    update_available: bool,
}

pub async fn status(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<BepInExStatus>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let status = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        if !instance.state.bepinex_installed {
            return Ok(BepInExStatus {
                installed: false,
                installed_version: None,
                latest_version: None,
                update_available: false,
            });
        }
        let latest = bepinex::latest_version(&db)?.version_number;
        let update_available = instance
            .state
            .bepinex_version
            .as_deref()
            .is_none_or(|installed| {
                bepinex::compare_versions(installed, &latest) == Ordering::Less
            });
        Ok(BepInExStatus {
            installed: true,
            installed_version: instance.state.bepinex_version,
            latest_version: Some(latest),
            update_available,
        })
    })
    .await?;
    Ok(Json(status))
}

pub async fn update(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<JobHandle>> {
    Ok(Json(spawn_update(&state, name).await?))
}

pub async fn spawn_update(state: &AppState, name: String) -> ApiResult<JobHandle> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let check_name = name.clone();
    let (instance_dir, from_version, to_version) = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &check_name)?;
        if !instance.state.bepinex_installed {
            return Err(BadRequest(format!("BepInEx is not installed on '{check_name}'")).into());
        }
        if crate::instance::lifecycle::is_running(&instance)? {
            return Err(InstanceError::ModsLocked(check_name).into());
        }
        let latest = bepinex::latest_version(&db)?.version_number;
        Ok((instance.dir, instance.state.bepinex_version, latest))
    })
    .await?;

    let transition = state
        .runtime
        .begin_transition(&name, InstanceTransition::UpdatingBepInEx)?;
    let db = state.db.clone();
    let activity = state.activity.clone();
    let job_name = name.clone();
    let job_from = from_version.clone();
    let id = state.jobs.spawn(
        JobKindDescr::BepInExUpdate {
            instance: name,
            from_version: from_version.clone(),
            to_version,
        },
        move |logger| {
            let _transition = transition;
            let outcome = bepinex::update_latest(
                &db,
                &job_name,
                &instance_dir,
                from_version.as_deref(),
                |line| logger.line(line),
            )?;
            if let UpdateOutcome::Updated { to, .. } = outcome {
                activity.record(
                    ActivityKind::BepInExUpdated {
                        from_version: job_from,
                        to_version: to,
                    },
                    Some(job_name),
                );
            }
            Ok(())
        },
    );
    Ok(JobHandle { id })
}
