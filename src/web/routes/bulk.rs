//! Fleet-wide operations that fan out over the existing single-instance
//! primitives — no new business logic, just a thin loop plus per-instance
//! success/failure reporting so one bad instance in a batch doesn't hide
//! how the rest went. Multi-instance is core to how Odin is used, but every
//! other route operates on one instance at a time; this is the one place
//! that isn't true.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::db::game_instances;
use crate::game::GameId;
use crate::instance::lifecycle;
use crate::web::error::{BadRequest, run_blocking};
use crate::web::routes::bepinex;
use crate::web::routes::mods::{JobHandle, spawn_mod_update_job};
use crate::web::runtime::InstanceTransition;
use crate::web::state::AppState;

/// Bounds one UUID-based bulk request and makes result allocation independent
/// from untrusted JSON input.
const MAX_BULK_INSTANCE_IDS: usize = 100;

#[derive(Deserialize)]
pub struct BulkRequest {
    pub names: Vec<String>,
}

#[derive(Serialize)]
pub struct BulkResult {
    pub name: String,
    pub ok: bool,
    pub error: Option<String>,
}

pub async fn bulk_start(
    State(state): State<AppState>,
    Json(req): Json<BulkRequest>,
) -> Json<Vec<BulkResult>> {
    let mut results = Vec::with_capacity(req.names.len());
    for name in req.names {
        let result = match state
            .runtime
            .begin_transition(&name, InstanceTransition::Starting)
        {
            Ok(_transition) => lifecycle::start(&state.paths, &state.db, &name).await,
            Err(error) => Err(error.into()),
        };
        results.push(BulkResult {
            ok: result.is_ok(),
            error: result.err().map(|e| format!("{e:#}")),
            name,
        });
    }
    Json(results)
}

pub async fn bulk_stop(
    State(state): State<AppState>,
    Json(req): Json<BulkRequest>,
) -> Json<Vec<BulkResult>> {
    let mut results = Vec::with_capacity(req.names.len());
    for name in req.names {
        let result = match state
            .runtime
            .begin_transition(&name, InstanceTransition::Stopping)
        {
            Ok(_transition) => lifecycle::stop(&state.paths, &state.db, &name).await,
            Err(error) => Err(error.into()),
        };
        results.push(BulkResult {
            ok: result.is_ok(),
            error: result.err().map(|e| format!("{e:#}")),
            name,
        });
    }
    Json(results)
}

pub async fn bulk_restart(
    State(state): State<AppState>,
    Json(req): Json<BulkRequest>,
) -> Json<Vec<BulkResult>> {
    let mut results = Vec::with_capacity(req.names.len());
    for name in req.names {
        let result = match state
            .runtime
            .begin_transition(&name, InstanceTransition::Restarting)
        {
            Ok(_transition) => lifecycle::restart(&state.paths, &state.db, &name).await,
            Err(error) => Err(error.into()),
        };
        results.push(BulkResult {
            ok: result.is_ok(),
            error: result.err().map(|e| format!("{e:#}")),
            name,
        });
    }
    Json(results)
}

// Not `ApiResult`-wrapped for the same reason `mods::update_mods` isn't:
// spawning a job can't fail synchronously, so there's nothing to wrap.
pub async fn bulk_update_mods(
    State(state): State<AppState>,
    Json(req): Json<BulkRequest>,
) -> Json<Vec<JobHandle>> {
    let handles = req
        .names
        .into_iter()
        .map(|name| JobHandle {
            id: spawn_mod_update_job(&state, name),
        })
        .collect();
    Json(handles)
}

#[derive(Serialize)]
pub struct BulkBepInExResult {
    pub name: String,
    pub job_id: Option<String>,
    pub error: Option<String>,
}

pub async fn bulk_update_bepinex(
    State(state): State<AppState>,
    Json(req): Json<BulkRequest>,
) -> Json<Vec<BulkBepInExResult>> {
    let mut results = Vec::with_capacity(req.names.len());
    for name in req.names {
        match bepinex::spawn_update(&state, name.clone()).await {
            Ok(handle) => results.push(BulkBepInExResult {
                name,
                job_id: Some(handle.id),
                error: None,
            }),
            Err(error) => results.push(BulkBepInExResult {
                name,
                job_id: None,
                error: Some(format!("{:#}", error.0)),
            }),
        }
    }
    Json(results)
}

#[derive(Deserialize)]
pub struct InstanceIdBulkRequest {
    pub ids: Vec<String>,
}

#[derive(Serialize)]
pub struct InstanceIdBulkResult {
    pub id: String,
    pub game: Option<GameId>,
    pub name: Option<String>,
    pub ok: bool,
    pub error: Option<String>,
    pub job_id: Option<String>,
}

/// Executes multi-game operations using the public UUID identity rather than
/// a mutable game/name pair.
pub async fn bulk_games_by_id(
    State(state): State<AppState>,
    axum::extract::Path(action): axum::extract::Path<String>,
    Json(req): Json<InstanceIdBulkRequest>,
) -> crate::web::error::ApiResult<Json<Vec<InstanceIdBulkResult>>> {
    use crate::web::routes::games;
    use axum::extract::Path;

    if !["start", "stop", "restart", "mods", "bepinex"].contains(&action.as_str())
        || req.ids.is_empty()
        || req.ids.len() > MAX_BULK_INSTANCE_IDS
    {
        return Err(BadRequest("Choose an operation and 1–100 instances".into()).into());
    }

    let mut results = Vec::with_capacity(MAX_BULK_INSTANCE_IDS);
    for id in req.ids {
        let db = state.db.clone();
        let lookup_id = id.clone();
        let identity =
            run_blocking(move || game_instances::identity_by_id(&db, &lookup_id)).await?;
        let Some(identity) = identity else {
            results.push(InstanceIdBulkResult {
                id,
                game: None,
                name: None,
                ok: false,
                error: Some("game instance does not exist".into()),
                job_id: None,
            });
            continue;
        };

        let result = match action.as_str() {
            "start" => games::start_instance_by_id(State(state.clone()), Path(id.clone()))
                .await
                .map(|_| None),
            "stop" => games::stop_instance_by_id(State(state.clone()), Path(id.clone()))
                .await
                .map(|_| None),
            "restart" => games::restart_instance_by_id(State(state.clone()), Path(id.clone()))
                .await
                .map(|_| None),
            _ if identity.game != GameId::Valheim => {
                Err(BadRequest("This game does not support mods".into()).into())
            }
            "mods" => Ok(Some(spawn_mod_update_job(&state, identity.name.clone()))),
            "bepinex" => bepinex::spawn_update(&state, identity.name.clone())
                .await
                .map(|job| Some(job.id)),
            _ => unreachable!(),
        };
        let (job_id, error) = match result {
            Ok(job_id) => (job_id, None),
            Err(error) => (None, Some(format!("{:#}", error.0))),
        };
        results.push(InstanceIdBulkResult {
            id,
            game: Some(identity.game),
            name: Some(identity.name),
            ok: error.is_none(),
            error,
            job_id,
        });
    }
    Ok(Json(results))
}
