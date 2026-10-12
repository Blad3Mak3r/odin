use std::path::{Component, Path as FsPath, PathBuf};

use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

use crate::game::GameId;
use crate::instance::InstanceError;
use crate::instance::lifecycle::LifecycleLock;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

/// Prefix of the staging file an upload is written to before being renamed
/// into place. Listings hide it so a half-written upload is never shown.
const UPLOAD_TMP_PREFIX: &str = ".odin-upload-";

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum SaveEntryKind {
    File,
    Dir,
}

#[derive(Debug, Serialize)]
pub struct SaveFileEntry {
    name: String,
    path: String,
    kind: SaveEntryKind,
    size_bytes: u64,
    modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct DirQuery {
    #[serde(default)]
    path: String,
}

#[derive(Debug, Error)]
pub enum SaveFileError {
    #[error("invalid save file path")]
    InvalidPath,
    #[error("save file '{0}' was not found")]
    NotFound(String),
}

/// Lists one directory of the instance's saves (the root when `path` is
/// empty): folders first, then files, each alphabetically.
pub async fn list_save_files(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Query(DirQuery { path }): Query<DirQuery>,
) -> ApiResult<Json<Vec<SaveFileEntry>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let root = save_root(&paths, &db, game, &name)?;
        let relative = relative_dir(&path)?;
        if !root.is_dir() && relative.as_os_str().is_empty() {
            return Ok(Vec::new());
        }
        let dir = open_dir(&root, &relative, &path)?;
        let mut entries = Vec::new();
        for entry in dir.entries()? {
            let entry = entry?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if file_name.starts_with(UPLOAD_TMP_PREFIX) {
                continue;
            }
            let metadata = entry.metadata()?;
            let kind = if metadata.is_dir() {
                SaveEntryKind::Dir
            } else if metadata.is_file() {
                SaveEntryKind::File
            } else {
                // Symlinks and special files are never exposed.
                continue;
            };
            entries.push(SaveFileEntry {
                path: relative
                    .join(&file_name)
                    .to_string_lossy()
                    .replace('\\', "/"),
                name: file_name,
                kind,
                size_bytes: metadata.len(),
                modified_at: metadata
                    .modified()
                    .ok()
                    .map(|time| DateTime::<Utc>::from(time.into_std())),
            });
        }
        entries.sort_by(|left, right| {
            matches!(right.kind, SaveEntryKind::Dir)
                .cmp(&matches!(left.kind, SaveEntryKind::Dir))
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(entries)
    })
    .await
    .map(Json)
}

pub async fn list_save_files_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    query: Query<DirQuery>,
) -> ApiResult<Json<Vec<SaveFileEntry>>> {
    let (game, name) = resolve_identity(&state, id).await?;
    list_save_files(State(state), Path((game, name)), query).await
}

/// Accepts one or more multipart files and stores them in the directory
/// `path`, replacing same-named files. Each file is streamed to a staging
/// file and renamed into place so a failed upload never leaves a partial
/// save behind. Only allowed while the server is stopped.
pub async fn upload_save_files(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Query(DirQuery { path }): Query<DirQuery>,
    mut multipart: Multipart,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    // The lock is held until the response is built so the server cannot be
    // started while files are being replaced.
    let (_lock, dir) = run_blocking(move || {
        let lock = ensure_stopped(&paths, &db, game, &name)?;
        let root = save_root(&paths, &db, game, &name)?;
        std::fs::create_dir_all(&root)?;
        let dir = open_dir(&root, &relative_dir(&path)?, &path)?;
        Ok((lock, Arc::new(dir)))
    })
    .await?;

    let mut uploaded = 0_usize;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| BadRequest(format!("invalid upload: {e}")))?
    {
        let Some(file_name) = field.file_name().map(str::to_owned) else {
            continue;
        };
        let target = upload_file_name(&file_name)?;
        if dir
            .symlink_metadata(&target)
            .is_ok_and(|metadata| metadata.is_dir())
        {
            return Err(BadRequest(format!("'{target}' is a folder")).into());
        }
        let staged = StagedUpload::new(&dir);
        let mut out = tokio::fs::File::from_std(dir.create(&staged.name)?.into_std());
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| BadRequest(format!("invalid upload: {e}")))?
        {
            out.write_all(&chunk).await?;
        }
        out.flush().await?;
        drop(out);
        dir.rename(&staged.name, &dir, &target)?;
        uploaded += 1;
    }
    if uploaded == 0 {
        return Err(BadRequest("no file was uploaded".to_string()).into());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn upload_save_files_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    query: Query<DirQuery>,
    multipart: Multipart,
) -> ApiResult<StatusCode> {
    let (game, name) = resolve_identity(&state, id).await?;
    upload_save_files(State(state), Path((game, name)), query, multipart).await
}

/// Deletes a file, or a folder with everything inside it. The save root
/// itself can never be deleted. Only allowed while the server is stopped.
pub async fn delete_save_file(
    State(state): State<AppState>,
    Path((game, name, requested)): Path<(GameId, String, String)>,
) -> ApiResult<StatusCode> {
    let relative = safe_relative_path(&requested)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let _lock = ensure_stopped(&paths, &db, game, &name)?;
        let root = save_root(&paths, &db, game, &name)?;
        let root = open_dir(&root, std::path::Path::new(""), "")?;
        let metadata = root
            .symlink_metadata(&relative)
            .map_err(|error| io_error(error, &requested))?;
        // A symlink is removed as a link; it is never followed.
        if metadata.is_dir() {
            root.remove_dir_all(&relative)
        } else {
            root.remove_file(&relative)
        }
        .map_err(|error| io_error(error, &requested))?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

pub async fn delete_save_file_by_id(
    State(state): State<AppState>,
    Path((id, requested)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let (game, name) = resolve_identity(&state, id).await?;
    delete_save_file(State(state), Path((game, name, requested))).await
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
    let (game, name) = resolve_identity(&state, id).await?;
    download_save_file(State(state), Path((game, name, requested))).await
}

async fn resolve_identity(state: &AppState, id: String) -> ApiResult<(GameId, String)> {
    let db = state.db.clone();
    run_blocking(move || {
        let identity = crate::db::game_instances::identity_by_id(&db, &id)?
            .ok_or_else(|| anyhow::anyhow!("game instance does not exist"))?;
        Ok((identity.game, identity.name))
    })
    .await
}

fn save_root(
    paths: &crate::paths::Paths,
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> anyhow::Result<PathBuf> {
    match game {
        GameId::ArkSurvivalAscended => {
            Ok(paths.game_instance_dir(game, name).join("Saved/SavedArks"))
        }
        GameId::Valheim => {
            let instance = crate::instance::Instance::load_existing(paths, db, name)?;
            Ok(crate::paths::instance_saves_dir(&instance.dir))
        }
        GameId::Rust => {
            let instance = crate::db::game_instances::load_rust(db, name)?
                .ok_or_else(|| InstanceError::NotFound(name.to_string()))?;
            Ok(crate::game::rust::backup_source(paths, &instance))
        }
        GameId::Palworld => Ok(paths
            .game_instance_dir(game, name)
            .join("runtime/Pal/Saved/SaveGames")),
        GameId::SevenDaysToDie => Ok(paths.game_instance_dir(game, name).join("Saves")),
    }
}

/// Takes the lifecycle lock (so the server cannot start mid-change) and
/// refuses the change while the server is running.
fn ensure_stopped(
    paths: &crate::paths::Paths,
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> anyhow::Result<LifecycleLock> {
    let lock = LifecycleLock::acquire(paths, game, name)?;
    if crate::game::instances::is_running(paths, db, game, name)? {
        return Err(BadRequest("stop the server before changing save files".into()).into());
    }
    Ok(lock)
}

/// A directory path relative to the save root; empty means the root.
fn relative_dir(value: &str) -> Result<PathBuf, SaveFileError> {
    if value.is_empty() {
        Ok(PathBuf::new())
    } else {
        safe_relative_path(value)
    }
}

/// Opens `relative` inside `root` through a capability handle, so neither
/// `..`, absolute paths nor symlinks can reach outside the save root.
fn open_dir(root: &FsPath, relative: &FsPath, requested: &str) -> anyhow::Result<Dir> {
    let root = Dir::open_ambient_dir(root, ambient_authority())
        .map_err(|error| io_error(error, requested))?;
    if relative.as_os_str().is_empty() {
        return Ok(root);
    }
    root.open_dir(relative)
        .map_err(|error| io_error(error, requested))
}

/// Maps the sandbox's refusal to leave the root to the same 400 the
/// download route gives.
fn io_error(error: std::io::Error, requested: &str) -> anyhow::Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => SaveFileError::NotFound(requested.to_string()).into(),
        std::io::ErrorKind::PermissionDenied => SaveFileError::InvalidPath.into(),
        _ => error.into(),
    }
}

/// An uploaded file name must be a single plain path component.
fn upload_file_name(value: &str) -> Result<String, SaveFileError> {
    let path = safe_relative_path(value)?;
    if path.components().count() != 1 || value.starts_with(UPLOAD_TMP_PREFIX) {
        return Err(SaveFileError::InvalidPath);
    }
    Ok(value.to_string())
}

/// Staging file for one upload; removed on drop unless it was renamed away.
struct StagedUpload {
    dir: Arc<Dir>,
    name: String,
}

impl StagedUpload {
    fn new(dir: &Arc<Dir>) -> Self {
        Self {
            dir: dir.clone(),
            name: format!("{UPLOAD_TMP_PREFIX}{}", Uuid::new_v4()),
        }
    }
}

impl Drop for StagedUpload {
    fn drop(&mut self) {
        if let Err(error) = self.dir.remove_file(&self.name)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(file = %self.name, %error, "failed to remove staged save upload");
        }
    }
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
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::db::Db;
    use crate::paths::Paths;

    struct Fixture {
        router: axum::Router,
        id: String,
        root: PathBuf,
        outside: PathBuf,
        paths: Paths,
        db: Arc<Db>,
    }

    fn fixture() -> Fixture {
        let dir = std::env::temp_dir().join(format!(
            "odin-saves-route-test-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let outside = dir.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        let paths = Paths {
            data_dir: dir.join("data"),
            config_dir: dir.join("config"),
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let instance =
            crate::db::game_instances::create_generic(&paths, &db, GameId::Palworld, "saves-test")
                .unwrap();
        let root = save_root(&paths, &db, GameId::Palworld, "saves-test").unwrap();
        std::fs::create_dir_all(&root).unwrap();
        Fixture {
            router: crate::web::router::build_router(AppState::new(paths.clone(), db.clone())),
            id: instance.identity.id,
            root,
            outside,
            paths,
            db,
        }
    }

    async fn send(fixture: &Fixture, request: Request<Body>) -> (StatusCode, String) {
        let response = fixture.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    fn get(fixture: &Fixture, query: &str) -> Request<Body> {
        Request::builder()
            .uri(format!("/api/instances/{}/saves{query}", fixture.id))
            .body(Body::empty())
            .unwrap()
    }

    fn delete(fixture: &Fixture, path: &str) -> Request<Body> {
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/instances/{}/saves/{path}", fixture.id))
            .body(Body::empty())
            .unwrap()
    }

    fn upload(fixture: &Fixture, query: &str, file_name: &str, content: &str) -> Request<Body> {
        let body = format!(
            "--B\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n{content}\r\n--B--\r\n"
        );
        Request::builder()
            .method("POST")
            .uri(format!("/api/instances/{}/saves{query}", fixture.id))
            .header("content-type", "multipart/form-data; boundary=B")
            .body(Body::from(body))
            .unwrap()
    }

    #[test]
    fn rejects_paths_that_can_escape_the_save_root() {
        assert!(safe_relative_path("worlds/world.db").is_ok());
        assert!(safe_relative_path("../odin.db").is_err());
        assert!(safe_relative_path("worlds/../odin.db").is_err());
        assert!(safe_relative_path("/etc/passwd").is_err());
        assert!(safe_relative_path("").is_err());
    }

    #[test]
    fn upload_file_name_accepts_only_a_plain_file_name() {
        assert!(upload_file_name("world.db").is_ok());
        assert!(upload_file_name("worlds/world.db").is_err());
        assert!(upload_file_name("../world.db").is_err());
        assert!(upload_file_name(".odin-upload-1").is_err());
    }

    #[tokio::test]
    async fn list_shows_one_directory_with_folders_first() {
        let fixture = fixture();
        std::fs::create_dir_all(fixture.root.join("worlds")).unwrap();
        std::fs::write(fixture.root.join("worlds/a.db"), "a").unwrap();
        std::fs::write(fixture.root.join("b.txt"), "b").unwrap();
        std::fs::write(fixture.root.join(".odin-upload-x"), "staged").unwrap();

        let (status, body) = send(&fixture, get(&fixture, "")).await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<_> = serde_json::from_str::<Vec<serde_json::Value>>(&body)
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["worlds", "b.txt"]);

        let (_, body) = send(&fixture, get(&fixture, "?path=worlds")).await;
        assert!(body.contains(r#""path":"worlds/a.db""#));
    }

    #[tokio::test]
    async fn list_rejects_paths_leaving_the_root() {
        let fixture = fixture();
        std::os::unix::fs::symlink(&fixture.outside, fixture.root.join("link")).unwrap();

        for query in ["?path=..%2Foutside", "?path=%2Fetc", "?path=link"] {
            let (status, _) = send(&fixture, get(&fixture, query)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        }
    }

    #[tokio::test]
    async fn upload_replaces_a_file_without_leaving_staging_files() {
        let fixture = fixture();
        std::fs::create_dir_all(fixture.root.join("worlds")).unwrap();

        for content in ["first", "second"] {
            let (status, _) =
                send(&fixture, upload(&fixture, "?path=worlds", "w.db", content)).await;
            assert_eq!(status, StatusCode::NO_CONTENT);
        }

        assert_eq!(
            std::fs::read_to_string(fixture.root.join("worlds/w.db")).unwrap(),
            "second"
        );
        assert_eq!(
            std::fs::read_dir(fixture.root.join("worlds"))
                .unwrap()
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn upload_rejects_unsafe_targets() {
        let fixture = fixture();
        std::os::unix::fs::symlink(&fixture.outside, fixture.root.join("link")).unwrap();
        std::fs::create_dir_all(fixture.root.join("folder")).unwrap();

        for (query, file_name) in [
            ("", "../escape.txt"),
            ("?path=link", "x.txt"),
            ("?path=..%2Foutside", "x.txt"),
            ("", "folder"),
        ] {
            let (status, _) = send(&fixture, upload(&fixture, query, file_name, "x")).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query} {file_name}");
        }
        assert!(!fixture.outside.join("x.txt").exists());
        assert!(!fixture.root.parent().unwrap().join("escape.txt").exists());
    }

    #[tokio::test]
    async fn writes_are_refused_while_the_server_is_running() {
        let fixture = fixture();
        std::fs::write(fixture.root.join("w.db"), "keep").unwrap();
        if !crate::podman::test_support::podman_reporting_running(&fixture.paths, &[&fixture.id]) {
            return;
        }
        let (paths, db) = (fixture.paths.clone(), fixture.db.clone());
        let running = tokio::task::spawn_blocking(move || {
            crate::game::instances::is_running(&paths, &db, GameId::Palworld, "saves-test")
        })
        .await
        .unwrap()
        .unwrap();
        assert!(running);

        let (status, body) = send(&fixture, upload(&fixture, "", "w.db", "new")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("stop the server"));
        let (status, _) = send(&fixture, delete(&fixture, "w.db")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("w.db")).unwrap(),
            "keep"
        );
    }

    #[tokio::test]
    async fn delete_removes_files_and_folders() {
        let fixture = fixture();
        std::fs::create_dir_all(fixture.root.join("worlds/nested")).unwrap();
        std::fs::write(fixture.root.join("worlds/nested/a.db"), "a").unwrap();
        std::fs::write(fixture.root.join("b.txt"), "b").unwrap();

        let (status, _) = send(&fixture, delete(&fixture, "b.txt")).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = send(&fixture, delete(&fixture, "worlds")).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(std::fs::read_dir(&fixture.root).unwrap().count(), 0);

        let (status, _) = send(&fixture, delete(&fixture, "missing")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(&fixture, delete(&fixture, "..%2Foutside")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_removes_a_symlink_without_touching_its_target() {
        let fixture = fixture();
        std::os::unix::fs::symlink(&fixture.outside, fixture.root.join("link")).unwrap();

        let (status, _) = send(&fixture, delete(&fixture, "link")).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(fixture.outside.join("secret.txt").exists());
        assert!(!fixture.root.join("link").exists());

        std::os::unix::fs::symlink(&fixture.outside, fixture.root.join("link")).unwrap();
        let (status, _) = send(&fixture, delete(&fixture, "link/secret.txt")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(fixture.outside.join("secret.txt").exists());
    }
}
