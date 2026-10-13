//! Global dashboard settings shared by every server instance.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::config::{GlobalConfig, PodmanConfig};
use crate::db::settings;
use crate::game::{GameId, RuntimeImage};
use crate::podman::PodmanClient;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::jobs::JobKindDescr;
use crate::web::routes::mods::JobHandle;
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct SettingsView {
    pub nexus_api_key_configured: bool,
    pub instance_defaults: settings::InstanceDefaults,
    pub config_file: String,
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

#[derive(Serialize)]
pub struct RuntimeImageView {
    pub role: RuntimeImage,
    pub reference: String,
    pub pinned: bool,
    pub games: Vec<GameId>,
    pub local_id: Option<String>,
    pub local_digest: Option<String>,
    pub created: Option<i64>,
}

pub async fn get_settings(State(state): State<AppState>) -> ApiResult<Json<SettingsView>> {
    let db = state.db.clone();
    let paths = state.paths.clone();
    let config_file = paths.config_file().display().to_string();
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
        config_file,
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
                    error: (!compatible).then(|| "Podman 5.0.0 or newer is required".to_string()),
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

pub async fn list_runtime_images(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<RuntimeImageView>>> {
    let paths = state.paths.clone();
    let config = run_blocking(move || Ok(GlobalConfig::load(&paths)?.podman)).await?;
    let Some(config) = config else {
        return Ok(Json(Vec::new()));
    };
    let client = PodmanClient::from_config(&config)?;
    let mut images = Vec::with_capacity(RuntimeImage::all().len());
    for role in RuntimeImage::all() {
        let reference = config.image(role);
        let local = client.inspect_image(&reference).await?;
        images.push(RuntimeImageView {
            role,
            pinned: !reference.ends_with(&format!(":{}", role.as_str())),
            games: crate::game::drivers()
                .into_iter()
                .filter(|game| game.runtime_image() == role)
                .map(|game| game.id())
                .collect(),
            reference,
            local_id: local.as_ref().map(|image| image.id.clone()),
            local_digest: local
                .as_ref()
                .and_then(|image| image.repo_digests.first().cloned()),
            created: local.map(|image| image.created),
        });
    }
    Ok(Json(images))
}

pub async fn pull_runtime_image(
    State(state): State<AppState>,
    Path(role): Path<RuntimeImage>,
) -> ApiResult<Json<JobHandle>> {
    let paths = state.paths.clone();
    let config = run_blocking(move || Ok(GlobalConfig::load(&paths)?.podman)).await?;
    let config = config.ok_or_else(|| anyhow::anyhow!("Podman is not configured"))?;
    let reference = config.image(role);
    let id = state
        .jobs
        .spawn(JobKindDescr::ImagePull { image: role }, move |logger| {
            logger.line(format!("Pulling {reference}"));
            PodmanClient::from_config(&config)?.pull_image_blocking(&reference)?;
            logger.line(format!("Pulled {reference}"));
            Ok(())
        });
    Ok(Json(JobHandle { id }))
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
