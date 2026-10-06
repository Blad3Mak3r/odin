use axum::Json;
use axum::extract::{Multipart, Path, State};
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
