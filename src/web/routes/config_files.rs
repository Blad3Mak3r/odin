use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::instance::Instance;
use crate::mods::config::{self, ConfigFileEntry};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct AdvancedConfigView {
    pub files: Vec<crate::game::config_documents::AdvancedConfigFile>,
}

#[derive(Deserialize)]
pub struct SetAdvancedConfigRequest {
    pub changes: Vec<crate::game::config_documents::AdvancedConfigChange>,
}

/// The declared, game-owned documents are deliberately separate from the
/// Valheim BepInEx file editor below.  This endpoint never follows arbitrary
/// paths and only returns values which Odin itself does not manage.
pub async fn list_advanced_config_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<AdvancedConfigView>> {
    let identity = crate::web::routes::games::resolve_instance_id(&state, &id).await?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let files = run_blocking(move || {
        if identity.game == crate::game::GameId::Rust {
            let instance = crate::db::game_instances::load_rust(&db, &identity.name)?
                .ok_or_else(|| anyhow::anyhow!("Rust instance does not exist"))?;
            return crate::game::config_documents::list_rust(&paths, &instance);
        }
        if !crate::db::game_instances::is_generic_game(identity.game) {
            return Ok(Vec::new());
        }
        let instance = crate::db::game_instances::load_generic(&db, identity.game, &identity.name)?
            .ok_or_else(|| anyhow::anyhow!("game instance does not exist"))?;
        crate::game::config_documents::list(&paths, &instance)
    })
    .await?;
    Ok(Json(AdvancedConfigView { files }))
}

pub async fn set_advanced_config_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<SetAdvancedConfigRequest>,
) -> ApiResult<StatusCode> {
    let identity = crate::web::routes::games::resolve_instance_id(&state, &id).await?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        if identity.game == crate::game::GameId::Rust {
            let instance = crate::db::game_instances::load_rust(&db, &identity.name)?
                .ok_or_else(|| anyhow::anyhow!("Rust instance does not exist"))?;
            return crate::game::config_documents::apply_rust(&paths, &instance, &request.changes)
                .map_err(|error| {
                    anyhow::Error::new(crate::web::error::BadRequest(error.to_string()))
                });
        }
        if !crate::db::game_instances::is_generic_game(identity.game) {
            return Err(anyhow::anyhow!(
                "this game has no declared advanced configuration"
            ));
        }
        let instance = crate::db::game_instances::load_generic(&db, identity.game, &identity.name)?
            .ok_or_else(|| anyhow::anyhow!("game instance does not exist"))?;
        crate::game::config_documents::apply(&paths, &instance, &request.changes)
            .map_err(|error| anyhow::Error::new(crate::web::error::BadRequest(error.to_string())))
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_config_files(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Vec<ConfigFileEntry>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let files = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        config::list(&instance.dir)
    })
    .await?;
    Ok(Json(files))
}

pub async fn list_config_files_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ConfigFileEntry>>> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    list_config_files(State(state), Path(name)).await
}

#[derive(Serialize)]
pub struct ConfigFileView {
    pub content: String,
}

pub async fn get_config_file(
    State(state): State<AppState>,
    Path((name, filename)): Path<(String, String)>,
) -> ApiResult<Json<ConfigFileView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let content = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        config::read(&instance.dir, &filename)
    })
    .await?;
    Ok(Json(ConfigFileView { content }))
}

pub async fn get_config_file_by_id(
    State(state): State<AppState>,
    Path((id, filename)): Path<(String, String)>,
) -> ApiResult<Json<ConfigFileView>> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    get_config_file(State(state), Path((name, filename))).await
}

#[derive(Deserialize)]
pub struct SetConfigFileRequest {
    pub content: String,
}

pub async fn set_config_file(
    State(state): State<AppState>,
    Path((name, filename)): Path<(String, String)>,
    Json(req): Json<SetConfigFileRequest>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        config::write(&instance.dir, &filename, &req.content)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_config_file_by_id(
    State(state): State<AppState>,
    Path((id, filename)): Path<(String, String)>,
    Json(req): Json<SetConfigFileRequest>,
) -> ApiResult<StatusCode> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    set_config_file(State(state), Path((name, filename)), Json(req)).await
}
