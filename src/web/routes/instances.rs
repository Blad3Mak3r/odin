use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::activity::ActivityKind;
use crate::db::Db;
use crate::instance::state::InstanceState;
use crate::instance::{self, Instance, lifecycle};
use crate::paths::Paths;
use crate::supervisor::client;
use crate::supervisor::protocol::Response;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::runtime::InstanceTransition;
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct InstanceView {
    #[serde(flatten)]
    pub state: InstanceState,
    pub running: bool,
    pub odin_version: Option<String>,
}

fn view(paths: &Paths, instance: Instance) -> anyhow::Result<InstanceView> {
    let running = lifecycle::is_running(&instance)?;
    let odin_version = if running {
        match client::ping_blocking(
            paths,
            crate::game::GameId::Valheim,
            &instance.state.name,
            Duration::from_millis(300),
        ) {
            Ok(Response::Pong { odin_version, .. }) => odin_version,
            _ => None,
        }
    } else {
        None
    };

    Ok(InstanceView {
        state: instance.state,
        running,
        odin_version,
    })
}

pub async fn list_instances(State(state): State<AppState>) -> ApiResult<Json<Vec<InstanceView>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let views = run_blocking(move || {
        instance::list_all(&paths, &db)?
            .into_iter()
            .map(|instance| view(&paths, instance))
            .collect::<anyhow::Result<Vec<_>>>()
    })
    .await?;
    Ok(Json(views))
}

#[derive(Deserialize)]
pub struct CreateInstanceRequest {
    pub name: String,
}

pub async fn create_instance(
    State(state): State<AppState>,
    Json(req): Json<CreateInstanceRequest>,
) -> ApiResult<Json<InstanceView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let name = req.name.clone();
    let created = run_blocking(move || {
        let crate::game::instances::GameInstance::Valheim(instance) =
            crate::game::instances::create(&paths, &db, crate::game::GameId::Valheim, &req.name)?
        else {
            unreachable!("Valheim creation returned a different game")
        };
        activity.record(ActivityKind::InstanceCreated, Some(name));
        Ok(instance)
    })
    .await?;
    let paths = state.paths.clone();
    let instance_view = run_blocking(move || view(&paths, created)).await?;
    Ok(Json(instance_view))
}

#[derive(Deserialize)]
pub struct CloneInstanceRequest {
    pub name: String,
    pub world_name: String,
}

pub async fn clone_instance(
    State(state): State<AppState>,
    Path(source_name): Path<String>,
    Json(req): Json<CloneInstanceRequest>,
) -> ApiResult<Json<InstanceView>> {
    // The transition guard serializes cloning with lifecycle and BepInEx
    // changes for the source, so its copied filesystem configuration is stable.
    let _transition = state
        .runtime
        .begin_transition(&source_name, InstanceTransition::Cloning)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let target_name = req.name.clone();
    let source_for_activity = source_name.clone();
    let activity = state.activity.clone();
    let cloned = run_blocking(move || {
        let cloned = instance::clone_configuration(
            &paths,
            &db,
            &source_name,
            &target_name,
            &req.world_name,
        )?;
        activity.record(
            ActivityKind::InstanceCloned {
                source: source_for_activity,
            },
            Some(cloned.state.name.clone()),
        );
        Ok(cloned)
    })
    .await?;
    let paths = state.paths.clone();
    let instance_view = run_blocking(move || view(&paths, cloned)).await?;
    Ok(Json(instance_view))
}

pub async fn get_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance_view = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        view(&paths, instance)
    })
    .await?;
    Ok(Json(instance_view))
}

#[derive(Deserialize)]
pub struct RenameRequest {
    pub new_name: String,
}

pub async fn rename_instance(
    State(state): State<AppState>,
    Path(old_name): Path<String>,
    Json(req): Json<RenameRequest>,
) -> ApiResult<Json<InstanceView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let renamed =
        run_blocking(move || lifecycle::rename(&paths, &db, &old_name, &req.new_name)).await?;
    let paths = state.paths.clone();
    let instance_view = run_blocking(move || view(&paths, renamed)).await?;
    Ok(Json(instance_view))
}

#[derive(Serialize)]
pub struct ConfigView {
    pub world_name: String,
    pub port: u16,
    pub password: Option<String>,
    pub public: bool,
    pub auto_restart: bool,
}

pub async fn get_config(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<ConfigView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance = run_blocking(move || Instance::load_existing(&paths, &db, &name)).await?;
    Ok(Json(ConfigView {
        world_name: instance.state.world_name,
        port: instance.state.port,
        password: instance.state.password,
        public: instance.state.public,
        auto_restart: instance.state.auto_restart,
    }))
}

pub async fn get_config_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ConfigView>> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    get_config(State(state), Path(name)).await
}

#[derive(Deserialize)]
pub struct ConfigUpdateRequest {
    pub world: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>,
    pub public: Option<bool>,
    pub auto_restart: Option<bool>,
}

pub async fn set_config(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<ConfigUpdateRequest>,
) -> ApiResult<Json<ConfigView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance = run_blocking(move || update_config(&paths, &db, &name, req)).await?;
    Ok(Json(ConfigView {
        world_name: instance.state.world_name,
        port: instance.state.port,
        password: instance.state.password,
        public: instance.state.public,
        auto_restart: instance.state.auto_restart,
    }))
}

pub async fn set_config_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ConfigUpdateRequest>,
) -> ApiResult<Json<ConfigView>> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    set_config(State(state), Path(name), Json(req)).await
}

fn update_config(
    paths: &Paths,
    db: &Db,
    name: &str,
    req: ConfigUpdateRequest,
) -> anyhow::Result<Instance> {
    let mut instance = Instance::load_existing(paths, db, name)?;

    if let Some(password) = &req.password
        && password.len() < 5
    {
        return Err(BadRequest(
            "password must be at least 5 characters (Valheim's own minimum)".to_string(),
        )
        .into());
    }

    if let Some(world) = req.world {
        instance.state.world_name = world;
    }
    if let Some(port) = req.port {
        instance.state.port = port;
    }
    if let Some(password) = req.password {
        instance.state.password = Some(password);
    }
    if let Some(public) = req.public {
        instance.state.public = public;
    }
    if let Some(auto_restart) = req.auto_restart {
        instance.state.auto_restart = auto_restart;
    }
    instance.save(db)?;
    Ok(instance)
}

#[derive(Deserialize)]
pub struct LogsQuery {
    #[serde(default = "default_log_lines")]
    pub lines: usize,
}

fn default_log_lines() -> usize {
    200
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn clone_route_creates_a_stopped_instance() {
        let dir = std::env::temp_dir().join(format!(
            "odin-clone-route-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        Instance::create(&paths, &db, "source").unwrap();
        let app = crate::web::router::build_router(AppState::new(paths.clone(), db.clone()));
        let request = Request::builder()
            .method("POST")
            .uri("/api/games/valheim/instances/source/clone")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"name":"target","world_name":"target-world"}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let target = Instance::load(&paths, &db, "target").unwrap().unwrap();
        assert_eq!(target.state.world_name, "target-world");
        assert!(!lifecycle::is_running(&target).unwrap());
    }

    #[tokio::test]
    async fn clone_route_rejects_a_source_in_transition() {
        let dir = std::env::temp_dir().join(format!(
            "odin-clone-transition-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        Instance::create(&paths, &db, "source").unwrap();
        let state = AppState::new(paths, db);
        let _transition = state
            .runtime
            .begin_transition("source", InstanceTransition::Starting)
            .unwrap();
        let app = crate::web::router::build_router(state);
        let request = Request::builder()
            .method("POST")
            .uri("/api/games/valheim/instances/source/clone")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"name":"target","world_name":"target-world"}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
}

#[derive(Serialize)]
pub struct LogsView {
    pub lines: Vec<String>,
}
