use axum::Json;
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::activity::ActivityKind;
use crate::game::GameId;
use crate::game::seven_days_to_die::{self, ModInfo};
use crate::instance::InstanceError;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::routes::mods::JobHandle;
use crate::web::state::AppState;

const MAX_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;

async fn resolve(state: &AppState, id: &str) -> ApiResult<String> {
    let db = state.db.clone();
    let id = id.to_string();
    run_blocking(move || {
        let identity = crate::db::game_instances::identity_by_id(&db, &id)?
            .ok_or_else(|| anyhow::anyhow!(InstanceError::NotFound(id)))?;
        anyhow::ensure!(
            identity.game == GameId::SevenDaysToDie,
            "instance is not a 7 Days to Die server"
        );
        Ok(identity.name)
    })
    .await
}

pub async fn list_mods(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ModInfo>>> {
    let name = resolve(&state, &id).await?;
    let paths = state.paths.clone();
    Ok(Json(
        run_blocking(move || seven_days_to_die::list(&paths, &name)).await?,
    ))
}

pub async fn upload_mod(
    State(state): State<AppState>,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<Json<JobHandle>> {
    let name = resolve(&state, &id).await?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let locked_name = name.clone();
    run_blocking(move || {
        if crate::game::instances::is_running(&paths, &db, GameId::SevenDaysToDie, &locked_name)? {
            return Err(InstanceError::ModsLocked(locked_name).into());
        }
        Ok(())
    })
    .await?;

    let mut replace = false;
    let mut zip_path = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| BadRequest(format!("invalid upload: {e}")))?
    {
        match field.name().unwrap_or_default() {
            "replace" => {
                replace = field
                    .text()
                    .await
                    .map_err(|e| BadRequest(format!("invalid replace field: {e}")))?
                    .trim()
                    == "true"
            }
            "file" => {
                let directory = state
                    .paths
                    .game_instance_dir(GameId::SevenDaysToDie, &name)
                    .join("Mods");
                tokio::fs::create_dir_all(&directory).await?;
                let destination =
                    directory.join(format!(".odin-upload-{}.zip", uuid::Uuid::new_v4()));
                let mut output = tokio::fs::File::create(&destination).await?;
                let mut received = 0u64;
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|e| BadRequest(format!("invalid upload: {e}")))?
                {
                    received = received
                        .checked_add(chunk.len() as u64)
                        .ok_or_else(|| BadRequest("upload is too large".into()))?;
                    if received > MAX_UPLOAD_BYTES {
                        tokio::fs::remove_file(&destination).await.ok();
                        return Err(BadRequest("mod ZIP must not exceed 512 MiB".into()).into());
                    }
                    output.write_all(&chunk).await?;
                }
                output.flush().await?;
                zip_path = Some(destination);
            }
            _ => {}
        }
    }
    let zip_path = zip_path.ok_or_else(|| BadRequest("a ZIP file is required".into()))?;
    let inspect_path = zip_path.clone();
    let info = match run_blocking(move || seven_days_to_die::inspect_archive(&inspect_path)).await {
        Ok(info) => info,
        Err(error) => {
            tokio::fs::remove_file(&zip_path).await.ok();
            return Err(error);
        }
    };
    let target = state
        .paths
        .game_instance_dir(GameId::SevenDaysToDie, &name)
        .join("Mods")
        .join(&info.name);
    if target.exists() && !replace {
        tokio::fs::remove_file(&zip_path).await.ok();
        return Err(InstanceError::AlreadyExists(format!(
            "mod '{}' already exists; confirm replacement and retry",
            info.name
        ))
        .into());
    }
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let job_name = name.clone();
    let id = state.jobs.spawn(
        crate::web::jobs::JobKindDescr::ModUpload {
            instance: name.clone(),
            name: info.display_name.clone(),
        },
        move |logger| {
            logger.line(format!("installing 7 Days to Die mod '{}'", info.name));
            let result = seven_days_to_die::install(&paths, &db, &job_name, &zip_path, replace);
            let _ = std::fs::remove_file(&zip_path);
            if let Ok(installed) = &result {
                logger.line(format!(
                    "installed {} {}",
                    installed.name, installed.version
                ));
                activity.record_for(
                    GameId::SevenDaysToDie,
                    ActivityKind::ModInstalled {
                        mod_id: installed.name.clone(),
                    },
                    Some(job_name.clone()),
                );
            }
            result.map(|_| ())
        },
    );
    Ok(Json(JobHandle { id }))
}

#[derive(Deserialize)]
pub struct ConsoleCommandRequest {
    pub command: String,
}

#[derive(Deserialize)]
pub struct PlayerActionRequest {
    #[serde(default)]
    pub reason: String,
}

fn player_target(target: &str) -> Result<&str, BadRequest> {
    if target.is_empty()
        || target.len() > 128
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(BadRequest(
            "player identifier may only contain letters, numbers, '_' or '-'".into(),
        ));
    }
    Ok(target)
}

fn command_reason(reason: &str) -> Result<String, BadRequest> {
    let reason = reason.trim();
    if reason.len() > 256 || reason.contains(['\r', '\n', '"']) {
        return Err(BadRequest(
            "reason must be a single line no longer than 256 characters".into(),
        ));
    }
    Ok(reason.to_string())
}

async fn console(
    state: AppState,
    id: String,
    command: String,
) -> ApiResult<Json<seven_days_to_die::ConsoleResponse>> {
    let name = resolve(&state, &id).await?;
    let db = state.db.clone();
    let paths = state.paths.clone();
    let response = run_blocking(move || {
        let instance = crate::db::game_instances::load_generic(&db, GameId::SevenDaysToDie, &name)?
            .ok_or_else(|| anyhow::anyhow!(InstanceError::NotFound(name)))?;
        if !instance.is_running() {
            anyhow::bail!("start the 7 Days to Die server before using its console");
        }
        seven_days_to_die::execute_console(&paths, &instance, &command)
    })
    .await?;
    Ok(Json(response))
}

pub async fn execute_console(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ConsoleCommandRequest>,
) -> ApiResult<Json<seven_days_to_die::ConsoleResponse>> {
    console(state, id, request.command).await
}

pub async fn list_players(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<seven_days_to_die::Player>>> {
    let name = resolve(&state, &id).await?;
    let output = console(state.clone(), id, "lpi".into()).await?.0.output;
    let players = seven_days_to_die::parse_players(&output);
    let db = state.db.clone();
    let tracked = players
        .iter()
        .map(|player| (player.name.clone(), player.platform_id.clone()))
        .collect::<Vec<_>>();
    run_blocking(move || {
        crate::db::player_sessions::sync_for_game(
            &db,
            GameId::SevenDaysToDie,
            &name,
            &tracked,
            Utc::now(),
        )
    })
    .await?;
    Ok(Json(players))
}

pub async fn player_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<crate::db::player_sessions::PlayerSession>>> {
    let name = resolve(&state, &id).await?;
    let db = state.db.clone();
    run_blocking(move || {
        crate::db::player_sessions::recent_for_game(&db, GameId::SevenDaysToDie, &name, 200)
    })
    .await
    .map(Json)
}

pub async fn kick_player(
    State(state): State<AppState>,
    Path((id, player)): Path<(String, String)>,
    Json(request): Json<PlayerActionRequest>,
) -> ApiResult<StatusCode> {
    let player = player_target(&player)?;
    let reason = command_reason(&request.reason)?;
    let command = if reason.is_empty() {
        format!("kick {player}")
    } else {
        format!("kick {player} {reason}")
    };
    let _ = console(state, id, command).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn ban_player(
    State(state): State<AppState>,
    Path((id, player)): Path<(String, String)>,
    Json(request): Json<PlayerActionRequest>,
) -> ApiResult<StatusCode> {
    let player = player_target(&player)?;
    let reason = command_reason(&request.reason)?;
    let command = if reason.is_empty() {
        format!("ban add {player} 0")
    } else {
        format!("ban add {player} 0 {reason}")
    };
    let _ = console(state, id, command).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unban_player(
    State(state): State<AppState>,
    Path((id, player)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let player = player_target(&player)?;
    let _ = console(state, id, format!("ban remove {player}")).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_admin(
    State(state): State<AppState>,
    Path((id, player)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let player = player_target(&player)?;
    let _ = console(state, id, format!("admin add {player} 0")).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_to_whitelist(
    State(state): State<AppState>,
    Path((id, player)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let player = player_target(&player)?;
    let _ = console(state, id, format!("whitelist add {player}")).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_admin(
    State(state): State<AppState>,
    Path((id, player)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let player = player_target(&player)?;
    let _ = console(state, id, format!("admin remove {player}")).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_from_whitelist(
    State(state): State<AppState>,
    Path((id, player)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let player = player_target(&player)?;
    let _ = console(state, id, format!("whitelist remove {player}")).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn access_list(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
) -> ApiResult<Json<seven_days_to_die::ConsoleResponse>> {
    let command = match kind.as_str() {
        "bans" => "ban list",
        "admins" => "admin list",
        "whitelist" => "whitelist list",
        _ => return Err(BadRequest("unknown 7 Days to Die access list".into()).into()),
    };
    console(state, id, command.into()).await
}
