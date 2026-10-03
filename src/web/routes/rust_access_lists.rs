//! Rust's owner, moderator, and ban lists.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::db::game_instances;
use crate::game::rust::access_lists::{self, RustAccessListKind};
use crate::web::error::{ApiError, ApiResult, run_blocking};
use crate::web::routes::lists::{AddListEntryRequest, ListView, SetListRequest};
use crate::web::state::AppState;

fn parse_kind(raw: &str) -> Result<RustAccessListKind, ApiError> {
    Ok(RustAccessListKind::parse(raw)?)
}

pub async fn get_list(
    State(state): State<AppState>,
    Path((name, kind)): Path<(String, String)>,
) -> ApiResult<Json<ListView>> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let ids = run_blocking(move || {
        let instance = game_instances::load_rust(&db, &name)?
            .ok_or_else(|| crate::instance::InstanceError::NotFound(name.clone()))?;
        access_lists::read(&paths, &instance, kind)
    })
    .await?;
    Ok(Json(ListView { ids }))
}

pub async fn set_list(
    State(state): State<AppState>,
    Path((name, kind)): Path<(String, String)>,
    Json(req): Json<SetListRequest>,
) -> ApiResult<StatusCode> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let instance = game_instances::load_rust(&db, &name)?
            .ok_or_else(|| crate::instance::InstanceError::NotFound(name.clone()))?;
        access_lists::write(&paths, &instance, kind, &req.ids)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_list_entry(
    State(state): State<AppState>,
    Path((name, kind)): Path<(String, String)>,
    Json(req): Json<AddListEntryRequest>,
) -> ApiResult<StatusCode> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let instance = game_instances::load_rust(&db, &name)?
            .ok_or_else(|| crate::instance::InstanceError::NotFound(name.clone()))?;
        access_lists::add_id(&paths, &instance, kind, &req.id)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_list_entry(
    State(state): State<AppState>,
    Path((name, kind, id)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let instance = game_instances::load_rust(&db, &name)?
            .ok_or_else(|| crate::instance::InstanceError::NotFound(name.clone()))?;
        access_lists::remove_id(&paths, &instance, kind, &id)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
