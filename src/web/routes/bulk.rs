//! Fleet-wide operations that fan out over the existing single-instance
//! primitives — no new business logic, just a thin loop plus per-instance
//! success/failure reporting so one bad instance in a batch doesn't hide
//! how the rest went. Multi-instance is core to how Odin is used, but every
//! other route operates on one instance at a time; this is the one place
//! that isn't true.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::instance::lifecycle;
use crate::web::routes::mods::{JobHandle, spawn_mod_update_job};
use crate::web::state::AppState;

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
        let result = lifecycle::start(&state.paths, &state.db, &name).await;
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
        let result = lifecycle::stop(&state.paths, &state.db, &name).await;
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
        let result = lifecycle::restart(&state.paths, &state.db, &name).await;
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
