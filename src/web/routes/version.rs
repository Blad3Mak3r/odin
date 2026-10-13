use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

use crate::db::Db;
use crate::game::GameId;
use crate::odin_update;
use crate::paths::Paths;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct VersionView {
    pub latest_version: Option<String>,
    pub latest_release_url: Option<String>,
    pub update_available: bool,
    pub outdated_instances: Vec<OutdatedInstance>,
    pub system_update_supported: bool,
    pub system_update_running: bool,
}

#[derive(Serialize)]
pub struct OutdatedInstance {
    pub game: GameId,
    pub name: String,
}

#[derive(Serialize)]
pub struct SystemUpdateStarted {
    pub started: bool,
}

pub async fn get_version(State(state): State<AppState>) -> ApiResult<Json<VersionView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let system_update = state.system_update.clone();
    let (status, outdated_instances, system_update_status) = run_blocking(move || {
        let status = odin_update::check(&db)?;
        let outdated_instances = outdated_running_instances(&paths, &db)?;
        let system_update_status = system_update.status()?;
        Ok((status, outdated_instances, system_update_status))
    })
    .await?;
    Ok(Json(VersionView {
        latest_version: status.latest_version,
        latest_release_url: status.latest_release_url,
        update_available: status.update_available,
        outdated_instances,
        system_update_supported: system_update_status.supported,
        system_update_running: system_update_status.running,
    }))
}

pub async fn start_system_update(
    State(state): State<AppState>,
) -> ApiResult<(StatusCode, Json<SystemUpdateStarted>)> {
    let system_update = state.system_update.clone();
    run_blocking(move || system_update.start()).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(SystemUpdateStarted { started: true }),
    ))
}

fn outdated_running_instances(paths: &Paths, db: &Db) -> anyhow::Result<Vec<OutdatedInstance>> {
    Ok(super::games::all_instance_views(paths, db)?
        .into_iter()
        .filter(|view| view.restart_required)
        .map(|view| OutdatedInstance {
            game: view.identity.game,
            name: view.identity.name,
        })
        .collect())
}
