use axum::Json;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::activity::ActivityKind;
use crate::mods::{self, GlobalMod, thunderstore};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::jobs::JobKindDescr;
use crate::web::state::AppState;

pub async fn list_global_mods(State(state): State<AppState>) -> ApiResult<Json<Vec<GlobalMod>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let mods = run_blocking(move || mods::list_global(&paths, &db)).await?;
    Ok(Json(mods))
}

pub async fn prune_mod(
    State(state): State<AppState>,
    Path(mod_id): Path<String>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || mods::prune_global(&paths, &db, &mod_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct InstalledModView {
    pub mod_id: String,
    pub version: String,
    pub installed_at: DateTime<Utc>,
    pub enabled: bool,
    pub icon: Option<String>,
}

pub async fn list_mods(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Vec<InstalledModView>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let views = run_blocking(move || {
        let installed = mods::list(&paths, &db, &name)?;
        let index = thunderstore::fetch_index(&db).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to fetch Thunderstore index; mods will show without icons");
            Vec::new()
        });
        Ok(installed
            .into_iter()
            .map(|m| InstalledModView {
                icon: thunderstore::find_icon(&index, &m.mod_id, &m.version),
                mod_id: m.mod_id,
                version: m.version,
                installed_at: m.installed_at,
                enabled: m.enabled,
            })
            .collect())
    })
    .await?;
    Ok(Json(views))
}

#[derive(Deserialize)]
pub struct AddModRequest {
    pub mod_id: String,
}

#[derive(Serialize)]
pub struct JobHandle {
    pub id: String,
}

// Returns a bare `Json<JobHandle>` rather than `ApiResult<...>` like other
// mutating routes: spawning a job onto the registry can't fail synchronously
// today, so there's nothing for `ApiResult` to wrap. This is intentional,
// not an oversight.
pub async fn add_mod(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<AddModRequest>,
) -> Json<JobHandle> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state.jobs.spawn(
        JobKindDescr::ModAdd {
            instance: name.clone(),
            mod_id: req.mod_id.clone(),
        },
        move |logger| {
            logger.line(format!("installing '{}' on '{}'", req.mod_id, name));
            let result = mods::add(&paths, &db, &name, &req.mod_id);
            if result.is_ok() {
                logger.line("done");
                activity.record(
                    ActivityKind::ModInstalled {
                        mod_id: req.mod_id.clone(),
                    },
                    Some(name.clone()),
                );
            }
            result
        },
    );
    Json(JobHandle { id })
}

pub async fn remove_mod(
    State(state): State<AppState>,
    Path((name, mod_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let event_name = name.clone();
    let event_mod_id = mod_id.clone();
    run_blocking(move || {
        mods::remove(&paths, &db, &name, &mod_id)?;
        activity.record(
            ActivityKind::ModRemoved {
                mod_id: event_mod_id,
            },
            Some(event_name),
        );
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn enable_mod(
    State(state): State<AppState>,
    Path((name, mod_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || mods::set_enabled(&paths, &db, &name, &mod_id, true)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn disable_mod(
    State(state): State<AppState>,
    Path((name, mod_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || mods::set_enabled(&paths, &db, &name, &mod_id, false)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// See the comment on `add_mod` above: intentionally not `ApiResult`-wrapped.
pub async fn update_mods(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<JobHandle> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state.jobs.spawn(
        JobKindDescr::ModUpdate {
            instance: name.clone(),
        },
        move |logger| {
            logger.line(format!("updating mods for '{name}'"));
            let result = mods::update(&paths, &db, &name);
            if result.is_ok() {
                logger.line("done");
                activity.record(ActivityKind::ModsUpdated, Some(name.clone()));
            }
            result
        },
    );
    Json(JobHandle { id })
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

#[derive(Serialize)]
pub struct ModSearchResult {
    pub mod_id: String,
    pub name: String,
    pub owner: String,
    pub version: String,
    pub description: String,
    pub icon: Option<String>,
    pub downloads: u64,
}

pub async fn search_mods(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Vec<ModSearchResult>>> {
    let db = state.db.clone();
    let results = run_blocking(move || {
        let index = thunderstore::fetch_index(&db)?;
        Ok(thunderstore::search(&index, &query.q)
            .into_iter()
            .filter_map(|pkg| {
                let version = pkg.versions.first()?;
                Some(ModSearchResult {
                    mod_id: format!("{}-{}", pkg.owner, pkg.name),
                    name: pkg.name.clone(),
                    owner: pkg.owner.clone(),
                    version: version.version_number.clone(),
                    description: version.description.clone(),
                    icon: version.icon.clone(),
                    downloads: version.downloads,
                })
            })
            .collect())
    })
    .await?;
    Ok(Json(results))
}

/// Accepts a user-uploaded mod `.zip` (multipart fields `name`, optional
/// `version`, `file`) and installs it exactly like a registry-installed
/// mod. The upload's bytes are streamed straight to a staging file rather
/// than buffered in memory — a modpack zip can be sizeable — then handed
/// off to a background job the same way `add_mod` does, since extraction
/// is comparable in cost to a registry download.
pub async fn upload_mod(
    State(state): State<AppState>,
    Path(name): Path<String>,
    mut multipart: Multipart,
) -> ApiResult<Json<JobHandle>> {
    let mut mod_name: Option<String> = None;
    let mut mod_version: Option<String> = None;
    let mut zip_path: Option<std::path::PathBuf> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| BadRequest(format!("invalid upload: {e}")))?
    {
        match field.name().unwrap_or_default() {
            "name" => {
                mod_name = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| BadRequest(format!("invalid 'name' field: {e}")))?,
                );
            }
            "version" => {
                mod_version = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| BadRequest(format!("invalid 'version' field: {e}")))?,
                );
            }
            "file" => {
                let dest = state
                    .paths
                    .mods_dir()
                    .join(format!(".upload-tmp-{}.zip", Uuid::new_v4()));
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let mut out = tokio::fs::File::create(&dest).await?;
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|e| BadRequest(format!("invalid upload: {e}")))?
                {
                    out.write_all(&chunk).await?;
                }
                out.flush().await?;
                zip_path = Some(dest);
            }
            _ => {}
        }
    }

    let mod_name = mod_name
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(|| BadRequest("a mod name is required".to_string()))?;
    let mod_version = mod_version
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
    let zip_path = zip_path.ok_or_else(|| BadRequest("no file was uploaded".to_string()))?;

    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state.jobs.spawn(
        JobKindDescr::ModUpload {
            instance: name.clone(),
            name: mod_name.clone(),
        },
        move |logger| {
            logger.line(format!("installing uploaded mod '{mod_name}' on '{name}'"));
            match mods::add_local(&paths, &db, &name, &mod_name, &mod_version, &zip_path) {
                Ok(mod_id) => {
                    logger.line("done");
                    activity.record(ActivityKind::ModInstalled { mod_id }, Some(name.clone()));
                    Ok(())
                }
                Err(e) => Err(e),
            }
        },
    );
    Ok(Json(JobHandle { id }))
}
