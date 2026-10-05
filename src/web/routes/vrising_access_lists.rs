use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::game::vrising::{self, VRisingAccessListKind};
use crate::web::error::{ApiError, ApiResult, run_blocking};
use crate::web::routes::lists::ListView;
use crate::web::state::AppState;

fn parse_kind(raw: &str) -> Result<VRisingAccessListKind, ApiError> {
    Ok(VRisingAccessListKind::parse(raw)?)
}

pub async fn get_list(
    State(state): State<AppState>,
    Path((name, kind)): Path<(String, String)>,
) -> ApiResult<Json<ListView>> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    let ids = run_blocking(move || vrising::read(&paths, &name, kind)).await?;
    Ok(Json(ListView { ids }))
}

#[derive(Deserialize)]
pub struct AddListEntryRequest {
    pub id: String,
}

pub async fn add_list_entry(
    State(state): State<AppState>,
    Path((name, kind)): Path<(String, String)>,
    Json(request): Json<AddListEntryRequest>,
) -> ApiResult<StatusCode> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    run_blocking(move || vrising::add_id(&paths, &name, kind, &request.id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_list_entry(
    State(state): State<AppState>,
    Path((name, kind, id)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    let kind = parse_kind(&kind)?;
    let paths = state.paths.clone();
    run_blocking(move || vrising::remove_id(&paths, &name, kind, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}
