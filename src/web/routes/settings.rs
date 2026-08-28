//! Global dashboard settings — currently just whether a Nexus Mods API key
//! has been configured. There's no multi-user/auth model in this app, so
//! this is a single shared key rather than something scoped per user.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::db::settings;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct SettingsView {
    pub nexus_api_key_configured: bool,
}

pub async fn get_settings(State(state): State<AppState>) -> ApiResult<Json<SettingsView>> {
    let db = state.db.clone();
    let configured = run_blocking(move || settings::get(&db, settings::NEXUS_API_KEY))
        .await?
        .is_some();
    Ok(Json(SettingsView {
        nexus_api_key_configured: configured,
    }))
}

#[derive(Deserialize)]
pub struct SetNexusApiKeyRequest {
    pub api_key: String,
}

pub async fn set_nexus_api_key(
    State(state): State<AppState>,
    Json(req): Json<SetNexusApiKeyRequest>,
) -> ApiResult<StatusCode> {
    let db = state.db.clone();
    run_blocking(move || settings::set(&db, settings::NEXUS_API_KEY, &req.api_key)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_nexus_api_key(State(state): State<AppState>) -> ApiResult<StatusCode> {
    let db = state.db.clone();
    run_blocking(move || settings::delete(&db, settings::NEXUS_API_KEY)).await?;
    Ok(StatusCode::NO_CONTENT)
}
