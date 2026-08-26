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
