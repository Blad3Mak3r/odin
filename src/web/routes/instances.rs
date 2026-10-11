use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::activity::ActivityKind;
use crate::db::Db;
use crate::instance::state::{InstanceState, ValheimModifiers, ValheimPreset, ValheimSetKey};
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

pub async fn clone_instance_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CloneInstanceRequest>,
) -> ApiResult<Json<InstanceView>> {
    let name = crate::web::routes::games::resolve_valheim_instance_name(&state, &id).await?;
    clone_instance(State(state), Path(name), Json(req)).await
}

#[derive(Deserialize)]
pub struct RenameRequest {
    pub new_name: String,
}

#[derive(Serialize)]
pub struct ConfigView {
    pub world_name: String,
    pub port: u16,
    pub password: Option<String>,
    pub public: bool,
    pub auto_restart: bool,
    pub save_interval: Option<u32>,
    pub backups: Option<u16>,
    pub backup_short: Option<u32>,
    pub backup_long: Option<u32>,
    pub crossplay: bool,
    pub playfab_instance_id: Option<String>,
    pub preset: Option<ValheimPreset>,
    pub modifiers: ValheimModifiers,
    pub set_keys: Vec<ValheimSetKey>,
}

fn config_view(state: &InstanceState) -> ConfigView {
    ConfigView {
        world_name: state.world_name.clone(),
        port: state.port,
        password: state.password.clone(),
        public: state.public,
        auto_restart: state.auto_restart,
        save_interval: state.save_interval,
        backups: state.backups,
        backup_short: state.backup_short,
        backup_long: state.backup_long,
        crossplay: state.crossplay,
        playfab_instance_id: state.playfab_instance_id.clone(),
        preset: state.preset,
        modifiers: state.modifiers.clone(),
        set_keys: state.set_keys.clone(),
    }
}

pub async fn get_config(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<ConfigView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance = run_blocking(move || Instance::load_existing(&paths, &db, &name)).await?;
    Ok(Json(config_view(&instance.state)))
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
    #[serde(default)]
    save_interval: OptionalUpdate<u32>,
    #[serde(default)]
    backups: OptionalUpdate<u16>,
    #[serde(default)]
    backup_short: OptionalUpdate<u32>,
    #[serde(default)]
    backup_long: OptionalUpdate<u32>,
    pub crossplay: Option<bool>,
    #[serde(default)]
    playfab_instance_id: OptionalUpdate<String>,
    #[serde(default)]
    preset: OptionalUpdate<ValheimPreset>,
    pub modifiers: Option<ValheimModifiers>,
    pub set_keys: Option<Vec<ValheimSetKey>>,
}

#[derive(Default)]
enum OptionalUpdate<T> {
    #[default]
    Unset,
    Set(Option<T>),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OptionalUpdate<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(Self::Set)
    }
}

pub async fn set_config(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<ConfigUpdateRequest>,
) -> ApiResult<Json<ConfigView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance = run_blocking(move || update_config(&paths, &db, &name, req)).await?;
    Ok(Json(config_view(&instance.state)))
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
    if let OptionalUpdate::Set(value) = req.save_interval {
        instance.state.save_interval = positive_optional("save interval", value)?;
    }
    if let OptionalUpdate::Set(value) = req.backups {
        instance.state.backups = positive_optional("backup count", value)?;
    }
    if let OptionalUpdate::Set(value) = req.backup_short {
        instance.state.backup_short = positive_optional("short backup interval", value)?;
    }
    if let OptionalUpdate::Set(value) = req.backup_long {
        instance.state.backup_long = positive_optional("long backup interval", value)?;
    }
    if let Some(crossplay) = req.crossplay {
        instance.state.crossplay = crossplay;
    }
    if let OptionalUpdate::Set(value) = req.playfab_instance_id {
        instance.state.playfab_instance_id = value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
    }
    if let OptionalUpdate::Set(value) = req.preset {
        instance.state.preset = value;
    }
    if let Some(modifiers) = req.modifiers {
        instance.state.modifiers = modifiers;
    }
    if let Some(mut set_keys) = req.set_keys {
        set_keys.dedup();
        instance.state.set_keys = set_keys;
    }
    instance.save(db)?;
    Ok(instance)
}

fn positive_optional<T>(label: &str, value: Option<T>) -> anyhow::Result<Option<T>>
where
    T: Copy + PartialEq + From<u8>,
{
    if value == Some(T::from(0)) {
        return Err(BadRequest(format!("{label} must be greater than zero")).into());
    }
    Ok(value)
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
        let source = Instance::create(&paths, &db, "source").unwrap();
        let source_id = crate::db::game_instances::ensure_valheim_identity(
            &db,
            "source",
            source.state.created_at,
        )
        .unwrap()
        .id;
        let app = crate::web::router::build_router(AppState::new(paths.clone(), db.clone()));
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/instances/{source_id}/valheim/clone"))
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
        let source = Instance::create(&paths, &db, "source").unwrap();
        let source_id = crate::db::game_instances::ensure_valheim_identity(
            &db,
            "source",
            source.state.created_at,
        )
        .unwrap()
        .id;
        let state = AppState::new(paths, db);
        let _transition = state
            .runtime
            .begin_transition("source", InstanceTransition::Starting)
            .unwrap();
        let app = crate::web::router::build_router(state);
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/instances/{source_id}/valheim/clone"))
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
