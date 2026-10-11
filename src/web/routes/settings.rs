//! Global dashboard settings shared by every server instance.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::config::{GlobalConfig, PodmanConfig};
use crate::db::settings;
use crate::podman::PodmanClient;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct SettingsView {
    pub nexus_api_key_configured: bool,
    pub instance_defaults: settings::InstanceDefaults,
    pub podman: PodmanSettingsView,
}

#[derive(Serialize)]
pub struct PodmanSettingsView {
    pub socket: Option<String>,
    pub runtime_image: Option<String>,
    pub status: PodmanStatusView,
}

#[derive(Serialize)]
pub struct PodmanStatusView {
    pub configured: bool,
    pub connected: bool,
    pub compatible: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

pub async fn get_settings(State(state): State<AppState>) -> ApiResult<Json<SettingsView>> {
    let db = state.db.clone();
    let paths = state.paths.clone();
    let (configured, instance_defaults, podman) = run_blocking(move || {
        Ok((
            settings::get(&db, settings::NEXUS_API_KEY)?.is_some(),
            settings::instance_defaults(&db)?,
            GlobalConfig::load(&paths)?.podman,
        ))
    })
    .await?;
    Ok(Json(SettingsView {
        nexus_api_key_configured: configured,
        instance_defaults,
        podman: podman_settings(podman).await,
    }))
}

async fn podman_settings(config: Option<PodmanConfig>) -> PodmanSettingsView {
    let Some(config) = config else {
        return PodmanSettingsView {
            socket: None,
            runtime_image: None,
            status: PodmanStatusView {
                configured: false,
                connected: false,
                compatible: false,
                version: None,
                error: None,
            },
        };
    };
    let socket = config.socket.display().to_string();
    let runtime_image = config.runtime_image.clone();
    let status = match PodmanClient::from_config(&config) {
        Ok(client) => match client.version().await {
            Ok(version) => {
                let compatible = version.is_supported();
                PodmanStatusView {
                    configured: true,
                    connected: true,
                    compatible,
                    version: Some(format!("{} (API {})", version.version, version.api_version)),
                    error: (!compatible).then(|| "Podman 4.0.0 or newer is required".to_string()),
                }
            }
            Err(error) => PodmanStatusView {
                configured: true,
                connected: false,
                compatible: false,
                version: None,
                error: Some(error.to_string()),
            },
        },
        Err(error) => PodmanStatusView {
            configured: true,
            connected: false,
            compatible: false,
            version: None,
            error: Some(error.to_string()),
        },
    };
    PodmanSettingsView {
        socket: Some(socket),
        runtime_image,
        status,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPodmanSettingsRequest {
    #[serde(default)]
    pub socket: Option<String>,
    #[serde(default)]
    pub runtime_image: Option<String>,
}

pub async fn set_podman_settings(
    State(state): State<AppState>,
    Json(request): Json<SetPodmanSettingsRequest>,
) -> ApiResult<Json<PodmanSettingsView>> {
    let paths = state.paths.clone();
    let socket = request.socket.and_then(|socket| {
        let socket = socket.trim().to_string();
        (!socket.is_empty()).then_some(socket)
    });
    let runtime_image = request.runtime_image.and_then(|image| {
        let image = image.trim().to_string();
        (!image.is_empty()).then_some(image)
    });
    let config = run_blocking(move || {
        let mut config = GlobalConfig::load(&paths)?;
        config.podman = socket.map(|socket| PodmanConfig {
            socket: socket.into(),
            runtime_image,
        });
        config.save(&paths)?;
        Ok(config.podman)
    })
    .await?;
    Ok(Json(podman_settings(config).await))
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
