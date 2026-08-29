use std::time::Duration;

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::db::Db;
use crate::instance::{self, lifecycle};
use crate::odin_update;
use crate::paths::Paths;
use crate::supervisor::client;
use crate::supervisor::protocol::Response;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

const SUPERVISOR_PING_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Serialize)]
pub struct VersionView {
    pub latest_version: Option<String>,
    pub latest_release_url: Option<String>,
    pub update_available: bool,
    pub outdated_instances: Vec<String>,
}

pub async fn get_version(State(state): State<AppState>) -> ApiResult<Json<VersionView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let (status, outdated_instances) = run_blocking(move || {
        let status = odin_update::check(&db)?;
        let outdated_instances = outdated_running_instances(&paths, &db)?;
        Ok((status, outdated_instances))
    })
    .await?;
    Ok(Json(VersionView {
        latest_version: status.latest_version,
        latest_release_url: status.latest_release_url,
        update_available: status.update_available,
        outdated_instances,
    }))
}

fn outdated_running_instances(paths: &Paths, db: &Db) -> anyhow::Result<Vec<String>> {
    let mut outdated = Vec::new();
    for instance in instance::list_all(paths, db)? {
        if !lifecycle::is_running(&instance)? {
            continue;
        }

        let Ok(Response::Pong { odin_version, .. }) =
            client::ping_blocking(paths, &instance.state.name, SUPERVISOR_PING_TIMEOUT)
        else {
            continue;
        };
        if supervisor_is_outdated(odin_version.as_deref(), env!("CARGO_PKG_VERSION")) {
            outdated.push(instance.state.name);
        }
    }
    Ok(outdated)
}

fn supervisor_is_outdated(supervisor_version: Option<&str>, current_version: &str) -> bool {
    supervisor_version.is_none_or(|version| odin_update::is_newer(current_version, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_supervisor_version_is_outdated() {
        assert!(supervisor_is_outdated(None, "0.8.0"));
    }

    #[test]
    fn lower_supervisor_version_is_outdated() {
        assert!(supervisor_is_outdated(Some("0.7.0"), "0.8.0"));
    }

    #[test]
    fn current_supervisor_version_is_not_outdated() {
        assert!(!supervisor_is_outdated(Some("0.8.0"), "0.8.0"));
    }

    #[test]
    fn newer_supervisor_version_is_not_outdated() {
        assert!(!supervisor_is_outdated(Some("0.9.0"), "0.8.0"));
    }
}
