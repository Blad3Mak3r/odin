//! Canonical multi-game API. Legacy `/instances` routes remain Valheim-only.

use anyhow::Context;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use sysinfo::Pid;

use crate::db::game_instances::{self, GameInstanceIdentity, GenericGameInstance, RustInstance};
use crate::game::{self, GameId, instances as game_instances_ops, rust};
use crate::instance::{self, Instance, lifecycle};
use crate::paths::Paths;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
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
    pub odin_version: Option<String>,
    pub capabilities: crate::game::GameCapabilities,
    pub config: Value,
}

#[derive(Deserialize)]
pub struct CreateGameInstanceRequest {
    pub name: String,
}

#[derive(Deserialize)]
pub struct DeleteGameInstanceQuery {
    #[serde(default)]
    pub keep_backups: bool,
}

#[derive(Deserialize)]
pub struct RustConfigUpdateRequest {
    pub port: Option<u16>,
    pub query_port: Option<u16>,
    pub rcon_port: Option<u16>,
    pub rcon_password: Option<String>,
    pub hostname: Option<String>,
    pub level: Option<String>,
    pub seed: Option<u32>,
    pub world_size: Option<u32>,
    pub max_players: Option<u16>,
    pub auto_restart: Option<bool>,
}

#[derive(Deserialize)]
pub struct GenericConfigUpdateRequest {
    pub port: u16,
    pub query_port: Option<u16>,
    pub admin_port: Option<u16>,
    pub settings: Value,
    pub auto_restart: bool,
}

#[derive(Deserialize)]
pub struct WipeRustMapRequest {
    pub confirmation: String,
}

#[derive(Deserialize)]
pub struct RconCommandRequest {
    pub command: String,
}

#[derive(Serialize)]
pub struct RconCommandResponse {
    pub output: String,
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
                GameId::VRising | GameId::Palworld | GameId::RunescapeDragonwilds => {}
            }
            let driver = game::driver(game);
            let install_dir = paths.game_install_dir(game);
            let log_file = paths
                .data_dir
                .join("logs")
                .join(format!("steamcmd-{}-install.log", driver.id()));
            let steamcmd = crate::steamcmd::SteamCmd::new(paths.steamcmd_dir());
            steamcmd.update_app_expect_file(
                driver.steam_app_id(),
                &install_dir,
                &log_file,
                install_dir.join(driver.server_binary()),
                |line| logger.line(line),
            )?;
            if game == GameId::Rust {
                logger.line("preparing Steamworks runtime");
                steamcmd.ensure_sdk64_client()?;
            }
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
        views.extend(rust_views(&paths, &db)?);
        for game in [
            GameId::VRising,
            GameId::Palworld,
            GameId::RunescapeDragonwilds,
        ] {
            views.extend(generic_views(&paths, &db, game)?);
        }
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
        GameId::Rust => rust_views(&paths, &db),
        GameId::VRising | GameId::Palworld | GameId::RunescapeDragonwilds => {
            generic_views(&paths, &db, game)
        }
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

/// Resolves the durable instance identity used by dashboard URLs. Names are
/// deliberately not accepted here: a rename must never invalidate a bookmark
/// or point a later operation at a different game with the same name.
pub async fn get_instance_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let view = run_blocking(move || {
        let identity =
            game_instances::identity_by_id(&db, &id)?.context("game instance does not exist")?;
        load_view(&paths, &db, identity.game, &identity.name)
    })
    .await?;
    Ok(Json(view))
}

pub async fn delete_instance(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Query(query): Query<DeleteGameInstanceQuery>,
) -> ApiResult<StatusCode> {
    delete_instance_for(state, game, name, query.keep_backups).await
}

pub async fn delete_instance_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteGameInstanceQuery>,
) -> ApiResult<StatusCode> {
    let identity = resolve_instance_id(&state, &id).await?;
    delete_instance_for(state, identity.game, identity.name, query.keep_backups).await
}

async fn delete_instance_for(
    state: AppState,
    game: GameId,
    name: String,
    keep_backups: bool,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let instance_name = name.clone();
    run_blocking(move || game_instances_ops::delete(&paths, &db, game, &name, keep_backups))
        .await?;
    state.runtime.remove_game_instance(game, &instance_name);
    state.activity.record_for(
        game,
        crate::activity::ActivityKind::InstanceDeleted,
        Some(instance_name),
    );
    Ok(StatusCode::NO_CONTENT)
}

pub async fn update_rust_config(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(request): Json<RustConfigUpdateRequest>,
) -> ApiResult<Json<ManagedInstanceView>> {
    update_rust_config_for(state, name, request).await
}

async fn update_rust_config_for(
    state: AppState,
    name: String,
    request: RustConfigUpdateRequest,
) -> ApiResult<Json<ManagedInstanceView>> {
    let db = state.db.clone();
    let paths = state.paths.clone();
    let view = run_blocking(move || {
        let _lock = lifecycle::LifecycleLock::acquire(&paths, GameId::Rust, &name)?;
        let instance =
            game_instances::load_rust(&db, &name)?.context("Rust instance does not exist")?;
        let mut config = instance.config;
        if let Some(port) = request.port {
            config.port = port;
        }
        if let Some(query_port) = request.query_port {
            config.query_port = query_port;
        }
        if let Some(rcon_port) = request.rcon_port {
            config.rcon_port = rcon_port;
        }
        if let Some(rcon_password) = request.rcon_password {
            config.rcon_password = rcon_password;
        }
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
        game_instances::update_rust_config(&db, &name, &config)
            .map(|instance| rust_view(&paths, instance))
    })
    .await?;
    Ok(Json(view))
}

pub async fn update_generic_config(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Json(request): Json<GenericConfigUpdateRequest>,
) -> ApiResult<Json<ManagedInstanceView>> {
    update_generic_config_for(state, game, name, request).await
}

async fn update_generic_config_for(
    state: AppState,
    game: GameId,
    name: String,
    request: GenericConfigUpdateRequest,
) -> ApiResult<Json<ManagedInstanceView>> {
    if !game_instances::is_generic_game(game) {
        return Err(BadRequest("this game has a dedicated configuration contract".into()).into());
    }
    let paths = state.paths.clone();
    let db = state.db.clone();
    let view = run_blocking(move || {
        let config = game_instances::GenericGameConfig {
            port: request.port,
            query_port: request.query_port,
            admin_port: request.admin_port,
            settings: request.settings,
            auto_restart: request.auto_restart,
        };
        game_instances::update_generic_config(&db, game, &name, &config)
            .map(|instance| generic_view(&paths, instance))
    })
    .await?;
    Ok(Json(view))
}

/// Canonical UUID configuration endpoint. The game-specific request remains
/// typed after identity resolution, so clients never have to include a game
/// or mutable name in their URL.
pub async fn update_config_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<Value>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let identity = resolve_instance_id(&state, &id).await?;
    match identity.game {
        GameId::Rust => {
            let request = serde_json::from_value(request)
                .map_err(|error| BadRequest(format!("invalid Rust configuration: {error}")))?;
            update_rust_config_for(state, identity.name, request).await
        }
        GameId::VRising | GameId::Palworld | GameId::RunescapeDragonwilds => {
            let request = serde_json::from_value(request)
                .map_err(|error| BadRequest(format!("invalid game configuration: {error}")))?;
            update_generic_config_for(state, identity.game, identity.name, request).await
        }
        GameId::Valheim => Err(BadRequest(
            "Valheim configuration remains on its dedicated compatibility endpoint".into(),
        )
        .into()),
    }
}

/// Sends one command to the Rust instance through its private loopback
/// WebRCON connection. The browser never receives the RCON password.
pub async fn execute_rust_rcon(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(request): Json<RconCommandRequest>,
) -> ApiResult<Json<RconCommandResponse>> {
    let command = request.command.trim().to_string();
    if command.is_empty() {
        return Err(BadRequest("Rust RCON command cannot be empty".to_string()).into());
    }
    if command.len() > 16 * 1024 {
        return Err(BadRequest("Rust RCON command must be at most 16 KiB".to_string()).into());
    }

    let db = state.db.clone();
    let name_for_load = name.clone();
    let instance = run_blocking(move || {
        game_instances::load_rust(&db, &name_for_load)?.context("Rust instance does not exist")
    })
    .await?;
    let output = rust::rcon::execute(&instance, &command).await?;
    Ok(Json(RconCommandResponse { output }))
}

pub async fn execute_rust_rcon_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<RconCommandRequest>,
) -> ApiResult<Json<RconCommandResponse>> {
    let identity = resolve_instance_id(&state, &id).await?;
    if identity.game != GameId::Rust {
        return Err(BadRequest("this API is only available for Rust instances".into()).into());
    }
    execute_rust_rcon(State(state), Path(identity.name), Json(request)).await
}

/// Executes V Rising's Source RCON command through the loopback listener
/// configured by Odin. Its password is never returned to API clients.
pub async fn execute_vrising_rcon_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<RconCommandRequest>,
) -> ApiResult<Json<RconCommandResponse>> {
    let command = request.command.trim().to_string();
    if command.is_empty() {
        return Err(BadRequest("V Rising RCON command cannot be empty".to_string()).into());
    }
    if command.len() > 16 * 1024 {
        return Err(BadRequest("V Rising RCON command must be at most 16 KiB".to_string()).into());
    }
    let identity = resolve_instance_id(&state, &id).await?;
    if identity.game != GameId::VRising {
        return Err(BadRequest("this API is only available for V Rising instances".into()).into());
    }
    let db = state.db.clone();
    let name = identity.name;
    let instance = run_blocking(move || {
        game_instances::load_generic(&db, GameId::VRising, &name)?
            .context("V Rising instance does not exist")
    })
    .await?;
    let output =
        run_blocking(move || crate::game::vrising::execute_rcon(&instance, &command)).await?;
    Ok(Json(RconCommandResponse { output }))
}

pub async fn wipe_rust_map(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(request): Json<WipeRustMapRequest>,
) -> ApiResult<Json<JobHandle>> {
    if request.confirmation != name {
        return Err(BadRequest("type the exact instance name to wipe its map".to_string()).into());
    }

    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state.jobs.spawn(
        JobKindDescr::MapWipe {
            instance: name.clone(),
        },
        move |logger| {
            logger.line(format!("wiping map for '{name}'"));
            let instance =
                game_instances::load_rust(&db, &name)?.context("Rust instance does not exist")?;
            let wiped = rust::wipe_map(&paths, &db, &instance)?;
            logger.line(format!("removed {wiped} world save file(s)"));
            activity.record_for(
                GameId::Rust,
                crate::activity::ActivityKind::MapWiped,
                Some(name),
            );
            logger.line("done");
            Ok(())
        },
    );
    Ok(Json(JobHandle { id }))
}

pub async fn full_wipe_rust(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(request): Json<WipeRustMapRequest>,
) -> ApiResult<Json<JobHandle>> {
    if request.confirmation != name {
        return Err(BadRequest("type the exact instance name to fully wipe it".to_string()).into());
    }

    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state.jobs.spawn(
        JobKindDescr::FullWipe {
            instance: name.clone(),
        },
        move |logger| {
            logger.line(format!("fully wiping '{name}'"));
            let instance =
                game_instances::load_rust(&db, &name)?.context("Rust instance does not exist")?;
            let wiped = rust::full_wipe(&paths, &db, &instance)?;
            logger.line(format!("removed {wiped} world and blueprint file(s)"));
            activity.record_for(
                GameId::Rust,
                crate::activity::ActivityKind::FullWiped,
                Some(name),
            );
            logger.line("done");
            Ok(())
        },
    );
    Ok(Json(JobHandle { id }))
}

/// Queues a Rust map wipe using the instance's durable UUID.
pub async fn wipe_rust_map_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<WipeRustMapRequest>,
) -> ApiResult<Json<JobHandle>> {
    let identity = resolve_instance_id(&state, &id).await?;
    if identity.game != GameId::Rust {
        return Err(BadRequest("this API is only available for Rust instances".into()).into());
    }
    wipe_rust_map(State(state), Path(identity.name), Json(request)).await
}

/// Queues a full Rust wipe using the instance's durable UUID.
pub async fn full_wipe_rust_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<WipeRustMapRequest>,
) -> ApiResult<Json<JobHandle>> {
    let identity = resolve_instance_id(&state, &id).await?;
    if identity.game != GameId::Rust {
        return Err(BadRequest("this API is only available for Rust instances".into()).into());
    }
    full_wipe_rust(State(state), Path(identity.name), Json(request)).await
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

pub async fn get_rust_resources_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<InstanceSnapshot>> {
    let identity = resolve_instance_id(&state, &id).await?;
    if identity.game != GameId::Rust {
        return Err(BadRequest("this API is only available for Rust instances".into()).into());
    }
    get_rust_resources(State(state), Path(identity.name)).await
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

pub async fn get_rust_resource_history_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<crate::web::routes::resources::HistoryQuery>,
) -> ApiResult<Json<Vec<ResourceSample>>> {
    let identity = resolve_instance_id(&state, &id).await?;
    if identity.game != GameId::Rust {
        return Err(BadRequest("this API is only available for Rust instances".into()).into());
    }
    get_rust_resource_history(State(state), Path(identity.name), Query(query)).await
}

pub async fn export_rust_resource_history(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<crate::web::routes::resources::HistoryQuery>,
) -> ApiResult<Response> {
    let db = state.db.clone();
    let load_name = name.clone();
    run_blocking(move || {
        game_instances::load_rust(&db, &load_name)?.context("Rust instance does not exist")
    })
    .await?;
    let hours = query.hours.unwrap_or(24 * 7);
    let since = chrono::Utc::now() - chrono::Duration::hours(hours as i64);
    let db = state.db.clone();
    let export_name = name.clone();
    let rows = run_blocking(move || {
        crate::db::resource_samples::range_for_instance(&db, GameId::Rust, &export_name, since)
    })
    .await?;
    Ok(crate::web::routes::resources::csv_response(
        &format!("rust-{name}-resources.csv"),
        &rows,
    ))
}

pub async fn export_rust_resource_history_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<crate::web::routes::resources::HistoryQuery>,
) -> ApiResult<Response> {
    let identity = resolve_instance_id(&state, &id).await?;
    if identity.game != GameId::Rust {
        return Err(BadRequest("this API is only available for Rust instances".into()).into());
    }
    export_rust_resource_history(State(state), Path(identity.name), Query(query)).await
}

pub async fn get_logs(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Query(query): Query<crate::web::routes::instances::LogsQuery>,
) -> ApiResult<Json<crate::web::routes::instances::LogsView>> {
    get_logs_for(state, game, name, query).await
}

pub async fn get_logs_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<crate::web::routes::instances::LogsQuery>,
) -> ApiResult<Json<crate::web::routes::instances::LogsView>> {
    let identity = resolve_instance_id(&state, &id).await?;
    get_logs_for(state, identity.game, identity.name, query).await
}

async fn get_logs_for(
    state: AppState,
    game: GameId,
    name: String,
    query: crate::web::routes::instances::LogsQuery,
) -> ApiResult<Json<crate::web::routes::instances::LogsView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let lines = run_blocking(move || {
        load_view(&paths, &db, game, &name)?;
        let log_file = crate::paths::instance_logs_dir(&paths.game_instance_dir(game, &name))
            .join("console.log");
        let mut lines = if log_file.is_file() {
            crate::commands::logs::read_tail(&log_file, query.lines)?
                .lines()
                .map(str::to_string)
                .collect()
        } else {
            Vec::new()
        };
        // Dragonwilds writes its Unreal server log independently from stdout.
        // Include it in Odin's normal log view rather than making operators
        // hunt through the isolated runtime tree after a failed start.
        if game == GameId::RunescapeDragonwilds {
            let native_log = paths
                .game_instance_dir(game, &name)
                .join("runtime/RSDragonwilds/Saved/Logs/RSDragonwilds.log");
            if native_log.is_file() {
                lines.extend(
                    crate::commands::logs::read_tail(&native_log, query.lines)?
                        .lines()
                        .map(|line| format!("[RSDragonwilds] {line}")),
                );
            }
        }
        Ok(lines)
    })
    .await?;
    Ok(Json(crate::web::routes::instances::LogsView { lines }))
}

pub async fn start_instance(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<ManagedInstanceView>> {
    start_instance_for(state, game, name).await
}

pub async fn start_instance_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let identity = resolve_instance_id(&state, &id).await?;
    start_instance_for(state, identity.game, identity.name).await
}

async fn start_instance_for(
    state: AppState,
    game: GameId,
    name: String,
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
    stop_instance_for(state, game, name).await
}

pub async fn stop_instance_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let identity = resolve_instance_id(&state, &id).await?;
    stop_instance_for(state, identity.game, identity.name).await
}

async fn stop_instance_for(state: AppState, game: GameId, name: String) -> ApiResult<StatusCode> {
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
    restart_instance_for(state, game, name).await
}

pub async fn restart_instance_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let identity = resolve_instance_id(&state, &id).await?;
    restart_instance_for(state, identity.game, identity.name).await
}

async fn restart_instance_for(
    state: AppState,
    game: GameId,
    name: String,
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

async fn resolve_instance_id(state: &AppState, id: &str) -> ApiResult<GameInstanceIdentity> {
    let db = state.db.clone();
    let id = id.to_string();
    run_blocking(move || {
        game_instances::identity_by_id(&db, &id)?.context("game instance does not exist")
    })
    .await
}

pub async fn list_backups(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<Vec<crate::backup::BackupEntry>>> {
    list_backups_for(state, game, name).await
}

pub async fn list_backups_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<crate::backup::BackupEntry>>> {
    let identity = resolve_instance_id(&state, &id).await?;
    list_backups_for(state, identity.game, identity.name).await
}

async fn list_backups_for(
    state: AppState,
    game: GameId,
    name: String,
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
    create_backup_for(state, game, name).await
}

pub async fn create_backup_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::backup::BackupEntry>> {
    let identity = resolve_instance_id(&state, &id).await?;
    create_backup_for(state, identity.game, identity.name).await
}

async fn create_backup_for(
    state: AppState,
    game: GameId,
    name: String,
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
    restore_backup_for(state, game, name, backup_id).await
}

pub async fn restore_backup_by_id(
    State(state): State<AppState>,
    Path((id, backup_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let identity = resolve_instance_id(&state, &id).await?;
    restore_backup_for(state, identity.game, identity.name, backup_id).await
}

async fn restore_backup_for(
    state: AppState,
    game: GameId,
    name: String,
    backup_id: String,
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
        game_instances_ops::GameInstance::Rust(instance) => Ok(rust_view(paths, instance)),
        game_instances_ops::GameInstance::Generic(instance) => Ok(generic_view(paths, instance)),
    }
}

fn valheim_views(paths: &Paths, db: &crate::db::Db) -> anyhow::Result<Vec<ManagedInstanceView>> {
    instance::list_all(paths, db)?
        .into_iter()
        .map(|instance| valheim_view(paths, db, instance))
        .collect()
}

fn valheim_view(
    paths: &Paths,
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
        odin_version: supervisor_version(paths, GameId::Valheim, &instance.state.name),
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

fn rust_views(paths: &Paths, db: &crate::db::Db) -> anyhow::Result<Vec<ManagedInstanceView>> {
    Ok(game_instances::list_rust(db)?
        .into_iter()
        .map(|instance| rust_view(paths, instance))
        .collect())
}

fn rust_view(paths: &Paths, instance: RustInstance) -> ManagedInstanceView {
    let running = instance.is_running();
    let odin_version = supervisor_version(paths, GameId::Rust, instance.name());
    ManagedInstanceView {
        identity: instance.identity,
        running,
        odin_version,
        capabilities: game::driver(GameId::Rust).capabilities(),
        config: serde_json::json!({
            "port": instance.config.port,
            "query_port": instance.config.query_port,
            "rcon_port": instance.config.rcon_port,
            "rcon_password": instance.config.rcon_password,
            "hostname": instance.config.hostname,
            "level": instance.config.level,
            "seed": instance.config.seed,
            "world_size": instance.config.world_size,
            "max_players": instance.config.max_players,
            "auto_restart": instance.config.auto_restart,
        }),
    }
}

fn generic_views(
    paths: &Paths,
    db: &crate::db::Db,
    game: GameId,
) -> anyhow::Result<Vec<ManagedInstanceView>> {
    Ok(game_instances::list_generic(db, game)?
        .into_iter()
        .map(|instance| generic_view(paths, instance))
        .collect())
}

fn generic_view(paths: &Paths, instance: GenericGameInstance) -> ManagedInstanceView {
    let game = instance.identity.game;
    let name = instance.identity.name.clone();
    let mut config = instance.config.settings.clone();
    if let Value::Object(values) = &mut config {
        for secret in [
            "admin_password",
            "world_password",
            "rcon_password",
            "password",
        ] {
            if values.contains_key(secret) {
                values.insert(secret.to_string(), Value::String(String::new()));
            }
        }
        values.insert("port".into(), serde_json::json!(instance.config.port));
        values.insert(
            "query_port".into(),
            serde_json::json!(instance.config.query_port),
        );
        values.insert(
            "admin_port".into(),
            serde_json::json!(instance.config.admin_port),
        );
        values.insert(
            "auto_restart".into(),
            serde_json::json!(instance.config.auto_restart),
        );
    }
    ManagedInstanceView {
        running: instance.is_running(),
        odin_version: supervisor_version(paths, game, &name),
        capabilities: game::driver(game).capabilities(),
        identity: instance.identity,
        config,
    }
}

fn supervisor_version(paths: &Paths, game: GameId, name: &str) -> Option<String> {
    match crate::supervisor::client::ping_blocking(paths, game, name, Duration::from_millis(300)) {
        Ok(crate::supervisor::protocol::Response::Pong { odin_version, .. }) => odin_version,
        _ => None,
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
        views.extend(rust_views(paths, db)?);
        Ok(views)
    }
}

#[derive(Deserialize)]
pub struct TagsRequest {
    pub tags: Vec<String>,
}

pub async fn set_tags(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Json(req): Json<TagsRequest>,
) -> ApiResult<StatusCode> {
    run_blocking(move || game_instances::set_tags(&state.db, game, &name, &req.tags)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_tags_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<TagsRequest>,
) -> ApiResult<StatusCode> {
    let identity = resolve_instance_id(&state, &id).await?;
    run_blocking(move || {
        game_instances::set_tags(&state.db, identity.game, &identity.name, &req.tags)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn rename_instance(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Json(req): Json<super::instances::RenameRequest>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let old = name.clone();
    let paths = state.paths.clone();
    let db = state.db.clone();
    let view = run_blocking(move || {
        let instance = game_instances_ops::rename(&paths, &db, game, &name, &req.new_name)?;
        game_instance_view(&paths, &db, instance)
    })
    .await?;
    state.runtime.remove_game_instance(game, &old);
    Ok(Json(view))
}

pub async fn rename_instance_by_id(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<super::instances::RenameRequest>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let identity = resolve_instance_id(&state, &id).await?;
    let old_name = identity.name.clone();
    let game = identity.game;
    let paths = state.paths.clone();
    let db = state.db.clone();
    let view = run_blocking(move || {
        let instance =
            game_instances_ops::rename(&paths, &db, game, &identity.name, &req.new_name)?;
        game_instance_view(&paths, &db, instance)
    })
    .await?;
    state.runtime.remove_game_instance(game, &old_name);
    Ok(Json(view))
}

pub async fn clone_rust_instance(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<CreateGameInstanceRequest>,
) -> ApiResult<Json<ManagedInstanceView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let view = run_blocking(move || {
        let instance = game_instances_ops::clone_rust(&paths, &db, &name, &req.name)?;
        activity.record_for(
            GameId::Rust,
            crate::activity::ActivityKind::InstanceCloned { source: name },
            Some(req.name),
        );
        Ok(rust_view(&paths, instance))
    })
    .await?;
    Ok(Json(view))
}
