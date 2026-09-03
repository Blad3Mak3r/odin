//! Canonical multi-game API. Legacy `/instances` routes remain Valheim-only.

use anyhow::Context;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sysinfo::Pid;

use crate::db::game_instances::{self, GameInstanceIdentity, RustInstance};
use crate::game::{self, GameId, instances as game_instances_ops};
use crate::instance::{self, Instance, lifecycle};
use crate::paths::Paths;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::jobs::JobKindDescr;
use crate::web::routes::mods::JobHandle;
use crate::web::runtime::{InstanceSnapshot, InstanceTransition, ResourceSample};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct GameView {
    pub id: GameId,
    pub name: &'static str,
    pub steam_app_id: &'static str,
    pub capabilities: crate::game::GameCapabilities,
}

#[derive(Serialize)]
pub struct ManagedInstanceView {
    #[serde(flatten)]
    pub identity: GameInstanceIdentity,
    pub running: bool,
    pub capabilities: crate::game::GameCapabilities,
    pub config: Value,
}

#[derive(Deserialize)]
pub struct CreateGameInstanceRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct RustConfigUpdateRequest {
    pub hostname: Option<String>,
    pub level: Option<String>,
    pub seed: Option<u32>,
    pub world_size: Option<u32>,
    pub max_players: Option<u16>,
    pub auto_restart: Option<bool>,
}

pub async fn list_games() -> Json<Vec<GameView>> {
    Json(
        game::drivers()
            .into_iter()
            .map(|driver| GameView {
                id: driver.id(),
                name: driver.display_name(),
                steam_app_id: driver.steam_app_id(),
                capabilities: driver.capabilities(),
            })
            .collect(),
    )
}

#[derive(Serialize)]
pub struct GameInstallStatusView {
    pub installed: bool,
    pub installed_build_id: Option<u64>,
    pub latest_build_id: Option<u64>,
    pub update_available: bool,
}

pub async fn get_install_status(
    State(state): State<AppState>,
    Path(game): Path<GameId>,
) -> ApiResult<Json<GameInstallStatusView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let status = run_blocking(move || crate::game::update::check(&paths, &db, game)).await?;
    Ok(Json(GameInstallStatusView {
        installed: status.installed_build_id.is_some(),
        installed_build_id: status.installed_build_id,
        latest_build_id: status.latest_build_id,
        update_available: status.update_available,
    }))
}

pub async fn install_game(
    State(state): State<AppState>,
    Path(game): Path<GameId>,
) -> Json<JobHandle> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state
        .jobs
        .spawn(JobKindDescr::SteamcmdInstall { game }, move |logger| {
            match game {
                GameId::Valheim => {
                    let running = instance::running_instance_names(&paths, &db)?;
                    if !running.is_empty() {
                        anyhow::bail!(
                            "refusing to update Valheim while instance(s) are running: {}",
                            running.join(", ")
                        );
                    }
                }
                GameId::Rust => {
                    let running = game_instances::list_rust(&db)?
                        .into_iter()
                        .filter(|instance| instance.is_running())
                        .map(|instance| instance.identity.name)
                        .collect::<Vec<_>>();
                    if !running.is_empty() {
                        anyhow::bail!(
                            "refusing to update Rust while instance(s) are running: {}",
                            running.join(", ")
                        );
                    }
                }
            }
            let driver = game::driver(game);
            let install_dir = paths.game_install_dir(game);
            let log_file = paths
                .data_dir
                .join("logs")
                .join(format!("steamcmd-{}-install.log", driver.id()));
            crate::steamcmd::SteamCmd::new(paths.steamcmd_dir()).update_app_expect_file(
                driver.steam_app_id(),
                &install_dir,
                &log_file,
                install_dir.join(driver.server_binary()),
                |line| logger.line(line),
            )?;
            activity.record_for(game, crate::activity::ActivityKind::ServerInstalled, None);
            logger.line("done");
            Ok(())
        });
    Json(JobHandle { id })
}

pub async fn list_all_instances(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<ManagedInstanceView>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let views = run_blocking(move || {
        let mut views = valheim_views(&paths, &db)?;
        views.extend(rust_views(&db)?);
        views.sort_by(|left, right| left.identity.name.cmp(&right.identity.name));
        Ok(views)
    })
    .await?;
    Ok(Json(views))
}

pub async fn list_instances(
    State(state): State<AppState>,
    Path(game): Path<GameId>,
) -> ApiResult<Json<Vec<ManagedInstanceView>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let views = run_blocking(move || match game {
        GameId::Valheim => valheim_views(&paths, &db),
        GameId::Rust => rust_views(&db),
    })
    .await?;
    Ok(Json(views))
}

pub async fn create_instance(
    State(state): State<AppState>,
    Path(game): Path<GameId>,
    Json(request): Json<CreateGameInstanceRequest>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let name = request.name.clone();
    let view = run_blocking(move || {
        game_instances_ops::create(&paths, &db, game, &request.name)
            .and_then(|instance| game_instance_view(&paths, &db, instance))
    })
    .await?;
    state.activity.record_for(
        game,
        crate::activity::ActivityKind::InstanceCreated,
        Some(name),
    );
    Ok(Json(view))
}

pub async fn get_instance(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let view = run_blocking(move || load_view(&paths, &db, game, &name)).await?;
    Ok(Json(view))
}

pub async fn update_rust_config(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(request): Json<RustConfigUpdateRequest>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let db = state.db.clone();
    let view = run_blocking(move || {
        let instance =
            game_instances::load_rust(&db, &name)?.context("Rust instance does not exist")?;
        let mut config = instance.config;
        if let Some(hostname) = request.hostname {
            config.hostname = hostname;
        }
        if let Some(level) = request.level {
            config.level = level;
        }
        if let Some(seed) = request.seed {
            config.seed = seed;
        }
        if let Some(world_size) = request.world_size {
            config.world_size = world_size;
        }
        if let Some(max_players) = request.max_players {
            config.max_players = max_players;
        }
        if let Some(auto_restart) = request.auto_restart {
            config.auto_restart = auto_restart;
        }
        game_instances::update_rust_config(&db, &name, &config).map(rust_view)
    })
    .await?;
    Ok(Json(view))
}

pub async fn get_rust_resources(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceSnapshot>> {
    let db = state.db.clone();
    let instance = run_blocking(move || {
        game_instances::load_rust(&db, &name)?.context("Rust instance does not exist")
    })
    .await?;
    Ok(Json(rust_resource_snapshot(&state, &instance)))
}

pub async fn get_rust_resource_history(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<crate::web::routes::resources::HistoryQuery>,
) -> ApiResult<Json<Vec<ResourceSample>>> {
    let db = state.db.clone();
    let load_name = name.clone();
    run_blocking(move || {
        game_instances::load_rust(&db, &load_name)?.context("Rust instance does not exist")
    })
    .await?;

    match query.hours {
        Some(hours) => {
            let db = state.db.clone();
            let since = chrono::Utc::now() - chrono::Duration::hours(hours as i64);
            let rows = run_blocking(move || {
                crate::db::resource_samples::range_for_instance(&db, GameId::Rust, &name, since)
            })
            .await?;
            Ok(Json(rows.into_iter().map(Into::into).collect()))
        }
        None => Ok(Json(
            state.runtime.game_instance_history(GameId::Rust, &name),
        )),
    }
}

pub async fn get_logs(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Query(query): Query<crate::web::routes::instances::LogsQuery>,
) -> ApiResult<Json<crate::web::routes::instances::LogsView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let lines = run_blocking(move || {
        load_view(&paths, &db, game, &name)?;
        let log_file = crate::paths::instance_logs_dir(&paths.game_instance_dir(game, &name))
            .join("console.log");
        if !log_file.is_file() {
            return Ok(Vec::new());
        }
        Ok(crate::commands::logs::read_tail(&log_file, query.lines)?
            .lines()
            .map(str::to_string)
            .collect())
    })
    .await?;
    Ok(Json(crate::web::routes::instances::LogsView { lines }))
}

pub async fn start_instance(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let _transition =
        state
            .runtime
            .begin_game_transition(game, &name, InstanceTransition::Starting)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let started = game_instances_ops::start(&paths, &db, game, &name).await?;
    let view = run_blocking(move || game_instance_view(&paths, &db, started)).await?;
    state.activity.record_for(
        game,
        crate::activity::ActivityKind::InstanceStarted,
        Some(name),
    );
    Ok(Json(view))
}

pub async fn stop_instance(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<StatusCode> {
    let _transition =
        state
            .runtime
            .begin_game_transition(game, &name, InstanceTransition::Stopping)?;
    game_instances_ops::stop(&state.paths, &state.db, game, &name).await?;
    state.activity.record_for(
        game,
        crate::activity::ActivityKind::InstanceStopped,
        Some(name),
    );
    Ok(StatusCode::NO_CONTENT)
}

pub async fn restart_instance(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let _transition =
        state
            .runtime
            .begin_game_transition(game, &name, InstanceTransition::Restarting)?;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let restarted = game_instances_ops::restart(&paths, &db, game, &name).await?;
    let view = run_blocking(move || game_instance_view(&paths, &db, restarted)).await?;
    state.activity.record_for(
        game,
        crate::activity::ActivityKind::InstanceStopped,
        Some(name.clone()),
    );
    state.activity.record_for(
        game,
        crate::activity::ActivityKind::InstanceStarted,
        Some(name),
    );
    Ok(Json(view))
}

pub async fn list_backups(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<Vec<crate::backup::BackupEntry>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let backups =
        run_blocking(move || game_instances_ops::list_backups(&paths, &db, game, &name)).await?;
    Ok(Json(backups))
}

pub async fn create_backup(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<crate::backup::BackupEntry>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance_name = name.clone();
    let backup =
        run_blocking(move || game_instances_ops::create_backup(&paths, &db, game, &name)).await?;
    state.activity.record_for(
        game,
        crate::activity::ActivityKind::BackupCreated {
            backup_id: backup.id.clone(),
        },
        Some(instance_name),
    );
    Ok(Json(backup))
}

pub async fn restore_backup(
    State(state): State<AppState>,
    Path((game, name, backup_id)): Path<(GameId, String, String)>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance_name = name.clone();
    let restored_backup_id = backup_id.clone();
    run_blocking(move || game_instances_ops::restore_backup(&paths, &db, game, &name, &backup_id))
        .await?;
    state.activity.record_for(
        game,
        crate::activity::ActivityKind::BackupRestored {
            backup_id: restored_backup_id,
        },
        Some(instance_name),
    );
    Ok(StatusCode::NO_CONTENT)
}

fn load_view(
    paths: &Paths,
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> anyhow::Result<ManagedInstanceView> {
    game_instances_ops::load(paths, db, game, name)
        .and_then(|instance| game_instance_view(paths, db, instance))
}

fn game_instance_view(
    paths: &Paths,
    db: &crate::db::Db,
    instance: game_instances_ops::GameInstance,
) -> anyhow::Result<ManagedInstanceView> {
    match instance {
        game_instances_ops::GameInstance::Valheim(instance) => valheim_view(paths, db, instance),
        game_instances_ops::GameInstance::Rust(instance) => Ok(rust_view(instance)),
    }
}

fn valheim_views(paths: &Paths, db: &crate::db::Db) -> anyhow::Result<Vec<ManagedInstanceView>> {
    instance::list_all(paths, db)?
        .into_iter()
        .map(|instance| valheim_view(paths, db, instance))
        .collect()
}

fn valheim_view(
    _paths: &Paths,
    db: &crate::db::Db,
    instance: Instance,
) -> anyhow::Result<ManagedInstanceView> {
    let identity = game_instances::ensure_valheim_identity(
        db,
        &instance.state.name,
        instance.state.created_at,
    )?;
    Ok(ManagedInstanceView {
        identity,
        running: lifecycle::is_running(&instance)?,
        capabilities: game::driver(GameId::Valheim).capabilities(),
        config: serde_json::json!({
            "world_name": instance.state.world_name,
            "port": instance.state.port,
            "password": instance.state.password,
            "public": instance.state.public,
            "auto_restart": instance.state.auto_restart,
        }),
    })
}

fn rust_views(db: &crate::db::Db) -> anyhow::Result<Vec<ManagedInstanceView>> {
    Ok(game_instances::list_rust(db)?
        .into_iter()
        .map(rust_view)
        .collect())
}

fn rust_view(instance: RustInstance) -> ManagedInstanceView {
    let running = instance.is_running();
    ManagedInstanceView {
        identity: instance.identity,
        running,
        capabilities: game::driver(GameId::Rust).capabilities(),
        config: serde_json::json!({
            "port": instance.config.port,
            "query_port": instance.config.query_port,
            "hostname": instance.config.hostname,
            "level": instance.config.level,
            "seed": instance.config.seed,
            "world_size": instance.config.world_size,
            "max_players": instance.config.max_players,
            "auto_restart": instance.config.auto_restart,
        }),
    }
}

pub(crate) fn rust_resource_snapshot(
    state: &AppState,
    instance: &RustInstance,
) -> InstanceSnapshot {
    if !instance.is_running() {
        return InstanceSnapshot::default();
    }

    let root_pids: Vec<u32> = instance.pid.into_iter().collect();
    let system = state.resources.lock().expect("resources lock poisoned");
    let mut cpu_percent = 0.0;
    let mut memory_bytes = 0;
    for pid in crate::instance::process::descendant_pids(&system, &root_pids) {
        if let Some(process) = system.process(Pid::from_u32(pid)) {
            cpu_percent += process.cpu_usage();
            memory_bytes += process.memory();
        }
    }
    InstanceSnapshot {
        running: true,
        ready: false,
        cpu_percent,
        memory_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn rust_and_valheim_can_use_the_same_name() {
        let dir = std::env::temp_dir().join(format!("odin-games-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        Instance::create(&paths, &db, "shared").unwrap();
        game_instances::create_rust(&paths, &db, "shared").unwrap();

        assert_eq!(list_all_for_test(&paths, &db).unwrap().len(), 2);
    }

    fn list_all_for_test(paths: &Paths, db: &Db) -> anyhow::Result<Vec<ManagedInstanceView>> {
        let mut views = valheim_views(paths, db)?;
        views.extend(rust_views(db)?);
        Ok(views)
    }
}
