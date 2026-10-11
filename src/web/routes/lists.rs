use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::instance::{Instance, lists};
use crate::web::error::{ApiError, ApiResult, run_blocking};
use crate::web::state::AppState;

fn parse_kind(raw: &str) -> Result<lists::ListKind, ApiError> {
    Ok(lists::ListKind::parse(raw)?)
}

#[derive(Serialize)]
pub struct ListView {
    pub ids: Vec<String>,
}

pub async fn get_list(
    State(state): State<AppState>,
    Path((name, kind)): Path<(String, String)>,
) -> ApiResult<Json<ListView>> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let ids = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        lists::read(&db, &instance, kind)
    })
    .await?;
    Ok(Json(ListView { ids }))
}

pub async fn get_list_by_id(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
) -> ApiResult<Json<ListView>> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    get_list(State(state), Path((name, kind))).await
}

#[derive(Deserialize)]
pub struct SetListRequest {
    pub ids: Vec<String>,
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
        let instance = Instance::load_existing(&paths, &db, &name)?;
        lists::write(&db, &instance, kind, &req.ids)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_list_by_id(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
    Json(req): Json<SetListRequest>,
) -> ApiResult<StatusCode> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    set_list(State(state), Path((name, kind)), Json(req)).await
}

#[derive(Deserialize)]
pub struct AddListEntryRequest {
    pub id: String,
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
        let instance = Instance::load_existing(&paths, &db, &name)?;
        lists::add_id(&db, &instance, kind, &req.id)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_list_entry_by_id(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
    Json(req): Json<AddListEntryRequest>,
) -> ApiResult<StatusCode> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    add_list_entry(State(state), Path((name, kind)), Json(req)).await
}

pub async fn remove_list_entry(
    State(state): State<AppState>,
    Path((name, kind, id)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        lists::remove_id(&db, &instance, kind, &id)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_list_entry_by_id(
    State(state): State<AppState>,
    Path((id, kind, entry_id)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    remove_list_entry(State(state), Path((name, kind, entry_id))).await
}
