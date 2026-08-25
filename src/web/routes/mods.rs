use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::mods::{self, GlobalMod, thunderstore};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::jobs::JobKindDescr;
use crate::web::state::AppState;

pub async fn list_global_mods(State(state): State<AppState>) -> ApiResult<Json<Vec<GlobalMod>>> {
    let paths = state.paths.clone();
    let mods = run_blocking(move || mods::list_global(&paths)).await?;
    Ok(Json(mods))
}

pub async fn prune_mod(
    State(state): State<AppState>,
    Path(mod_id): Path<String>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    run_blocking(move || mods::prune_global(&paths, &mod_id)).await?;
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
    let views = run_blocking(move || {
        let installed = mods::list(&paths, &name)?;
        let index = thunderstore::fetch_index(&paths).unwrap_or_else(|e| {
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
    let id = state.jobs.spawn(
        JobKindDescr::ModAdd {
            instance: name.clone(),
            mod_id: req.mod_id.clone(),
        },
        move |logger| {
            logger.line(format!("installing '{}' on '{}'", req.mod_id, name));
            let result = mods::add(&paths, &name, &req.mod_id);
            if result.is_ok() {
                logger.line("done");
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
    run_blocking(move || mods::remove(&paths, &name, &mod_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn enable_mod(
    State(state): State<AppState>,
    Path((name, mod_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    run_blocking(move || mods::set_enabled(&paths, &name, &mod_id, true)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn disable_mod(
    State(state): State<AppState>,
    Path((name, mod_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    run_blocking(move || mods::set_enabled(&paths, &name, &mod_id, false)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// See the comment on `add_mod` above: intentionally not `ApiResult`-wrapped.
pub async fn update_mods(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<JobHandle> {
    let paths = state.paths.clone();
    let id = state.jobs.spawn(
        JobKindDescr::ModUpdate {
            instance: name.clone(),
        },
        move |logger| {
            logger.line(format!("updating mods for '{name}'"));
            let result = mods::update(&paths, &name);
            if result.is_ok() {
                logger.line("done");
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
    let paths = state.paths.clone();
    let results = run_blocking(move || {
        let index = thunderstore::fetch_index(&paths)?;
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
