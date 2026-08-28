use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::activity::ActivityKind;
use crate::db::Db;
use crate::instance::state::InstanceState;
use crate::instance::{self, Instance, InstanceError, lifecycle};
use crate::paths::Paths;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct InstanceView {
    #[serde(flatten)]
    pub state: InstanceState,
    pub running: bool,
}

fn view(instance: Instance) -> anyhow::Result<InstanceView> {
    let running = lifecycle::is_running(&instance)?;
    Ok(InstanceView {
        state: instance.state,
        running,
    })
}

pub async fn list_instances(State(state): State<AppState>) -> ApiResult<Json<Vec<InstanceView>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let views = run_blocking(move || {
        instance::list_all(&paths, &db)?
            .into_iter()
            .map(view)
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
        let instance = Instance::create(&paths, &db, &req.name)?;
        activity.record(ActivityKind::InstanceCreated, Some(name));
        Ok(instance)
    })
    .await?;
    Ok(Json(view(created)?))
}

pub async fn get_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance = run_blocking(move || Instance::load_existing(&paths, &db, &name)).await?;
    Ok(Json(view(instance)?))
}

pub async fn start_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceView>> {
    let started = lifecycle::start(&state.paths, &state.db, &name).await?;
    Ok(Json(view(started)?))
}

pub async fn stop_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    lifecycle::stop(&state.paths, &state.db, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn restart_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceView>> {
    let restarted = lifecycle::restart(&state.paths, &state.db, &name).await?;
    Ok(Json(view(restarted)?))
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
    Ok(Json(view(renamed)?))
}

#[derive(Deserialize)]
pub struct DeleteQuery {
    #[serde(default)]
    pub keep_backups: bool,
}

pub async fn delete_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let delete_name = name.clone();
    run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &delete_name)?;
        if lifecycle::is_running(&instance)? {
            return Err(InstanceError::AlreadyRunning(delete_name).into());
        }
        lifecycle::delete(&db, &instance, query.keep_backups)?;
        activity.record(ActivityKind::InstanceDeleted, Some(delete_name));
        Ok(())
    })
    .await?;
    state.runtime.remove_instance(&name);
    Ok(StatusCode::NO_CONTENT)
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

#[derive(Serialize)]
pub struct LogsView {
    pub lines: Vec<String>,
}

pub async fn get_logs(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<LogsQuery>,
) -> ApiResult<Json<LogsView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let tail = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &db, &name)?;
        let log_file = crate::paths::instance_logs_dir(&instance.dir).join("console.log");
        if !log_file.is_file() {
            return Ok(String::new());
        }
        Ok(crate::commands::logs::read_tail(&log_file, query.lines)?)
    })
    .await?;
    Ok(Json(LogsView {
        lines: tail.lines().map(str::to_string).collect(),
    }))
}
