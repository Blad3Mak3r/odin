//! Global dashboard settings shared by every server instance.

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
    pub instance_defaults: settings::InstanceDefaults,
}

pub async fn get_settings(State(state): State<AppState>) -> ApiResult<Json<SettingsView>> {
    let db = state.db.clone();
    let (configured, instance_defaults) = run_blocking(move || {
        Ok((
            settings::get(&db, settings::NEXUS_API_KEY)?.is_some(),
            settings::instance_defaults(&db)?,
        ))
    })
    .await?;
    Ok(Json(SettingsView {
        nexus_api_key_configured: configured,
        instance_defaults,
    }))
}

pub async fn set_instance_defaults(
    State(state): State<AppState>,
    Json(defaults): Json<settings::InstanceDefaults>,
) -> ApiResult<Json<settings::InstanceDefaults>> {
    if defaults.backup_interval_hours == 0 {
        return Err(crate::web::error::BadRequest(
            "backup_interval_hours must be at least 1".to_string(),
        )
        .into());
    }
    if defaults.backup_retain_count == 0 {
        return Err(crate::web::error::BadRequest(
            "backup_retain_count must be at least 1".to_string(),
        )
        .into());
    }
    let db = state.db.clone();
    let saved = defaults.clone();
    run_blocking(move || settings::set_instance_defaults(&db, &defaults)).await?;
    Ok(Json(saved))
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
