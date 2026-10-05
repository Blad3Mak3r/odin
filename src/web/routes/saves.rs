use std::path::{Component, Path as FsPath, PathBuf};

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;
use tokio::io::AsyncReadExt;

use crate::game::GameId;
use crate::instance::InstanceError;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

#[derive(Debug, Serialize)]
pub struct SaveFileEntry {
    path: String,
    size_bytes: u64,
    modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Error)]
pub enum SaveFileError {
    #[error("invalid save file path")]
    InvalidPath,
    #[error("save file '{0}' was not found")]
    NotFound(String),
}

pub async fn list_save_files(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<Vec<SaveFileEntry>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let root = save_root(&paths, &db, game, &name)?;
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        collect_files(&root, &root, &mut files)?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(files)
    })
    .await
    .map(Json)
}

pub async fn list_save_files_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<SaveFileEntry>>> {
    let db = state.db.clone();
    let identity = run_blocking(move || {
        crate::db::game_instances::identity_by_id(&db, &id)?
            .ok_or_else(|| anyhow::anyhow!("game instance does not exist"))
    })
    .await?;
    list_save_files(State(state), Path((identity.game, identity.name))).await
}

pub async fn download_save_file(
    State(state): State<AppState>,
    Path((game, name, requested)): Path<(GameId, String, String)>,
) -> ApiResult<Response> {
    let relative = safe_relative_path(&requested)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let requested_for_error = requested.clone();
    let file_path = run_blocking(move || {
        let root = save_root(&paths, &db, game, &name)?;
        let canonical_root = root
            .canonicalize()
            .map_err(|_| SaveFileError::NotFound(requested_for_error.clone()))?;
        let candidate = root.join(&relative);
        let canonical_file = candidate
            .canonicalize()
            .map_err(|_| SaveFileError::NotFound(requested_for_error.clone()))?;
        if !canonical_file.starts_with(&canonical_root) || !canonical_file.is_file() {
            return Err(SaveFileError::InvalidPath.into());
        }
        Ok(canonical_file)
    })
    .await?;

    let mut file = tokio::fs::File::open(&file_path)
        .await
        .map_err(|_| SaveFileError::NotFound(requested.clone()))?;
    let metadata = file
        .metadata()
        .await
        .map_err(|_| SaveFileError::NotFound(requested.clone()))?;
    let stream = async_stream::stream! {
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            match file.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => yield Ok::<Vec<u8>, std::io::Error>(buffer[..read].to_vec()),
                Err(error) => {
                    yield Err(error);
                    break;
                }
            }
        }
    };
    let filename = file_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("save-file")
        .replace(['"', '\\'], "_");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from_stream(stream))
        .map_err(Into::into)
}

pub async fn download_save_file_by_id(
    State(state): State<AppState>,
    Path((id, requested)): Path<(String, String)>,
) -> ApiResult<Response> {
    let db = state.db.clone();
    let identity = run_blocking(move || {
        crate::db::game_instances::identity_by_id(&db, &id)?
            .ok_or_else(|| anyhow::anyhow!("game instance does not exist"))
    })
    .await?;
    download_save_file(
        State(state),
        Path((identity.game, identity.name, requested)),
    )
    .await
}

fn save_root(
    paths: &crate::paths::Paths,
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> anyhow::Result<PathBuf> {
    match game {
        GameId::Valheim => {
            let instance = crate::instance::Instance::load_existing(paths, db, name)?;
            Ok(crate::paths::instance_saves_dir(&instance.dir))
        }
        GameId::Rust => {
            let instance = crate::db::game_instances::load_rust(db, name)?
                .ok_or_else(|| InstanceError::NotFound(name.to_string()))?;
            Ok(crate::game::rust::backup_source(paths, &instance))
        }
        GameId::VRising => Ok(paths.game_instance_dir(game, name).join("data/Saves")),
        GameId::Palworld => Ok(paths
            .game_instance_dir(game, name)
            .join("runtime/Pal/Saved/SaveGames")),
        GameId::RunescapeDragonwilds => Ok(paths
            .game_instance_dir(game, name)
            .join("runtime/RSDragonwilds/Saved/Savegames")),
    }
}

fn collect_files(
    root: &FsPath,
    directory: &FsPath,
    files: &mut Vec<SaveFileEntry>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.path().symlink_metadata()?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_files(root, &entry.path(), files)?;
        } else if metadata.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(SaveFileEntry {
                path: relative,
                size_bytes: metadata.len(),
                modified_at: metadata.modified().ok().map(DateTime::<Utc>::from),
            });
        }
    }
    Ok(())
}

fn safe_relative_path(value: &str) -> Result<PathBuf, SaveFileError> {
    let path = FsPath::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SaveFileError::InvalidPath);
    }
    Ok(path.to_path_buf())
}

impl IntoResponse for SaveFileError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::InvalidPath => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
        };
        (
            status,
            Json(serde_json::json!({ "error": self.to_string() })),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_can_escape_the_save_root() {
        assert!(safe_relative_path("worlds/world.db").is_ok());
        assert!(safe_relative_path("../odin.db").is_err());
        assert!(safe_relative_path("worlds/../odin.db").is_err());
        assert!(safe_relative_path("/etc/passwd").is_err());
        assert!(safe_relative_path("").is_err());
    }
}
