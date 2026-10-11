//! Odin-owned routes for Palworld's local administrative REST service.

use anyhow::{Context, Result, bail};
use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::Value;

use crate::db::game_instances::{self, GenericGameInstance};
use crate::game::{GameId, palworld};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

#[derive(Deserialize)]
pub struct MessageRequest {
    pub message: String,
}

#[derive(Deserialize)]
pub struct PlayerRequest {
    pub user_id: String,
    pub message: Option<String>,
}

#[derive(Deserialize)]
pub struct UnbanRequest {
    pub user_id: String,
}

#[derive(Deserialize)]
pub struct ShutdownRequest {
    pub wait_time: Option<u32>,
    pub message: Option<String>,
}

async fn instance_for_id(state: &AppState, id: String) -> ApiResult<GenericGameInstance> {
    let db = state.db.clone();
    run_blocking(move || load_instance(&db, &id)).await
}

fn load_instance(db: &crate::db::Db, id: &str) -> Result<GenericGameInstance> {
    let identity =
        game_instances::identity_by_id(db, id)?.context("game instance does not exist")?;
    if identity.game != GameId::Palworld {
        bail!(BadRequest(
            "this API is only available for Palworld instances".into()
        ));
    }
    game_instances::load_generic(db, GameId::Palworld, &identity.name)?
        .context("Palworld instance disappeared")
}

pub async fn players(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let instance = instance_for_id(&state, id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || palworld::players(&paths, &instance)).await?,
    ))
}

pub async fn metrics(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let instance = instance_for_id(&state, id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || palworld::metrics(&paths, &instance)).await?,
    ))
}

pub async fn announce(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<MessageRequest>,
) -> ApiResult<Json<Value>> {
    let message = validate_message(request.message)?;
    let instance = instance_for_id(&state, id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || palworld::announce(&paths, &instance, &message)).await?,
    ))
}

pub async fn save(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let instance = instance_for_id(&state, id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || palworld::save(&paths, &instance)).await?,
    ))
}

pub async fn kick(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<PlayerRequest>,
) -> ApiResult<Json<Value>> {
    let request = validate_player_request(request)?;
    let instance = instance_for_id(&state, id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || {
            palworld::kick(
                &paths,
                &instance,
                &request.user_id,
                request.message.as_deref(),
            )
        })
        .await?,
    ))
}

pub async fn ban(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<PlayerRequest>,
) -> ApiResult<Json<Value>> {
    let request = validate_player_request(request)?;
    let instance = instance_for_id(&state, id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || {
            palworld::ban(
                &paths,
                &instance,
                &request.user_id,
                request.message.as_deref(),
            )
        })
        .await?,
    ))
}

pub async fn unban(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UnbanRequest>,
) -> ApiResult<Json<Value>> {
    if request.user_id.trim().is_empty() {
        return Err(BadRequest("Palworld user ID is required".into()).into());
    }
    let instance = instance_for_id(&state, id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || palworld::unban(&paths, &instance, &request.user_id)).await?,
    ))
}

pub async fn shutdown(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ShutdownRequest>,
) -> ApiResult<Json<Value>> {
    let instance = instance_for_id(&state, id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || {
            palworld::shutdown(
                &paths,
                &instance,
                request.wait_time,
                request.message.as_deref(),
            )
        })
        .await?,
    ))
}

fn validate_message(message: String) -> ApiResult<String> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(BadRequest("message is required".into()).into());
    }
    if message.len() > 1024 {
        return Err(BadRequest("message must be at most 1024 characters".into()).into());
    }
    Ok(message)
}

fn validate_player_request(mut request: PlayerRequest) -> ApiResult<PlayerRequest> {
    request.user_id = request.user_id.trim().to_string();
    if request.user_id.is_empty() {
        return Err(BadRequest("Palworld user ID is required".into()).into());
    }
    if let Some(message) = request.message.take() {
        request.message = Some(validate_message(message)?);
    }
    Ok(request)
}
