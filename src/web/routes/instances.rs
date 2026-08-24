use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

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
    let views = run_blocking(move || {
        instance::list_all(&paths)?
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
    let created = run_blocking(move || Instance::create(&paths, &req.name)).await?;
    Ok(Json(view(created)?))
}

pub async fn get_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceView>> {
    let paths = state.paths.clone();
    let instance = run_blocking(move || Instance::load_existing(&paths, &name)).await?;
    Ok(Json(view(instance)?))
}

pub async fn start_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceView>> {
    let paths = state.paths.clone();
    let started = run_blocking(move || lifecycle::start(&paths, &name)).await?;
    Ok(Json(view(started)?))
}

pub async fn stop_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    run_blocking(move || lifecycle::stop(&paths, &name)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn restart_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceView>> {
    let paths = state.paths.clone();
    let restarted = run_blocking(move || lifecycle::restart(&paths, &name)).await?;
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
    let renamed = run_blocking(move || lifecycle::rename(&paths, &old_name, &req.new_name)).await?;
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
    run_blocking(move || delete_instance_dir(&paths, &name, query.keep_backups)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Same behavior as `commands::delete::run`, minus the interactive
/// confirmation prompt — the frontend asks for confirmation itself.
fn delete_instance_dir(paths: &Paths, name: &str, keep_backups: bool) -> anyhow::Result<()> {
    let instance = Instance::load_existing(paths, name)?;
    if lifecycle::is_running(&instance)? {
        return Err(InstanceError::AlreadyRunning(name.to_string()).into());
    }

    if keep_backups {
        for entry in std::fs::read_dir(&instance.dir)? {
            let entry = entry?;
            if entry.file_name() == "backups" {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            }?;
        }
    } else {
        std::fs::remove_dir_all(&instance.dir)?;
    }

    Ok(())
}

#[derive(Serialize)]
pub struct ConfigView {
    pub world_name: String,
    pub port: u16,
    pub password: Option<String>,
    pub public: bool,
}

pub async fn get_config(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<ConfigView>> {
    let paths = state.paths.clone();
    let instance = run_blocking(move || Instance::load_existing(&paths, &name)).await?;
    Ok(Json(ConfigView {
        world_name: instance.state.world_name,
        port: instance.state.port,
        password: instance.state.password,
        public: instance.state.public,
    }))
}

#[derive(Deserialize)]
pub struct ConfigUpdateRequest {
    pub world: Option<String>,
    pub port: Option<u16>,
    pub password: Option<String>,
    pub public: Option<bool>,
}

pub async fn set_config(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<ConfigUpdateRequest>,
) -> ApiResult<Json<ConfigView>> {
    let paths = state.paths.clone();
    let instance = run_blocking(move || update_config(&paths, &name, req)).await?;
    Ok(Json(ConfigView {
        world_name: instance.state.world_name,
        port: instance.state.port,
        password: instance.state.password,
        public: instance.state.public,
    }))
}

fn update_config(paths: &Paths, name: &str, req: ConfigUpdateRequest) -> anyhow::Result<Instance> {
    let mut instance = Instance::load_existing(paths, name)?;

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
    instance.save()?;
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
    let tail = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &name)?;
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
