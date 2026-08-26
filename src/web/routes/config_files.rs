use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::instance::Instance;
use crate::mods::config::{self, ConfigFileEntry};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

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
