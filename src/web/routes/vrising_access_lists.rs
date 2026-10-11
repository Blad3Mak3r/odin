use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::game::vrising::{self, VRisingAccessListKind};
use crate::web::error::{ApiError, ApiResult, run_blocking};
use crate::web::routes::lists::ListView;
use crate::web::state::AppState;

async fn instance_name_for_id(state: &AppState, id: String) -> ApiResult<String> {
    let db = state.db.clone();
    run_blocking(move || {
        let identity = crate::db::game_instances::identity_by_id(&db, &id)?
            .ok_or_else(|| anyhow::anyhow!("game instance does not exist"))?;
        anyhow::ensure!(
            identity.game == crate::game::GameId::VRising,
            "this API is only available for V Rising instances"
        );
        Ok(identity.name)
    })
    .await
}

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

pub async fn get_list_by_id(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
) -> ApiResult<Json<ListView>> {
    let name = instance_name_for_id(&state, id).await?;
    get_list(State(state), Path((name, kind))).await
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

pub async fn add_list_entry_by_id(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
    Json(request): Json<AddListEntryRequest>,
) -> ApiResult<StatusCode> {
    let name = instance_name_for_id(&state, id).await?;
    add_list_entry(State(state), Path((name, kind)), Json(request)).await
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

pub async fn remove_list_entry_by_id(
    State(state): State<AppState>,
    Path((id, kind, entry_id)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    let name = instance_name_for_id(&state, id).await?;
    remove_list_entry(State(state), Path((name, kind, entry_id))).await
}
