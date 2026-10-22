//! Embedded web dashboard: a JSON API plus the built frontend, served from a
//! single async task started by `odin serve`. Everything else in the crate
//! is synchronous — this module (and `commands::serve`) is the only place
//! async/tokio is used.

mod audit;
mod auth;
mod backup_scheduler;
mod error;
mod image_monitor;
pub mod jobs;
mod log_tail;
mod players;
mod retention;
mod router;
pub mod routes;
mod runtime;
mod sse;
mod state;
mod static_files;
mod uptime_scheduler;
mod webhooks;
mod world_saves;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::future::join_all;
use tokio::task::AbortHandle;

use crate::activity::ActivityKind;
use crate::db::Db;
use crate::db::game_instances;
use crate::game::GameId;
use crate::instance;
use crate::paths::Paths;
use crate::podman::dispatch;
use crate::podman::lifecycle::StopReason;
use routes::resources::{compute_host_snapshot, compute_instance_snapshot};
use runtime::InstanceTransition;
use runtime::{InstanceResourceEntry, ResourcesTick};
use state::AppState;

const STOP_INSTANCES_ON_SHUTDOWN_ENV: &str = "ODIN_STOP_INSTANCES_ON_SHUTDOWN";

pub async fn serve(paths: Paths, addr: SocketAddr) -> Result<()> {
    let db = Arc::new(Db::open(&paths).context("failed to open database")?);
    if let Err(error) = crate::podman::startup::fail_interrupted_attempts(&db) {
        tracing::warn!(%error, "could not close interrupted startup attempts");
    }
    let config = crate::config::GlobalConfig::load(&paths)?;
    let public_origin = config
        .public_origin()
        .context("invalid dashboard public_url")?;
    let trusted_proxies = config
        .trusted_proxies()
        .context("invalid dashboard trusted_proxies")?;
    let state = AppState::new(paths, db)
        .with_public_origin(public_origin)
        .with_trusted_proxies(trusted_proxies);
    auth::arm_bootstrap(&state).context("failed to prepare dashboard sign-in")?;
    retention::run_once(&state);
    retention::spawn(state.clone());
    webhooks::spawn(state.clone());
    spawn_telemetry(state.clone());
    backup_scheduler::spawn(state.clone());
    uptime_scheduler::spawn(state.clone());
    image_monitor::spawn(state.clone());

    let router = router::build_app(state.clone());
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    tracing::info!(%addr, "odin dashboard listening");
    println!("Odin dashboard listening on http://{addr}");
    if let Some(public) = &state.public_origin {
        println!("Public URL: {}", public.origin);
    }
    if let Some(ip) = local_network_ip() {
        println!("Network: http://{ip}:{}", addr.port());
    }

    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    );
    if !stop_instances_on_shutdown() {
        return server.await.context("web server error");
    }

    tokio::select! {
        result = server => result.context("web server error"),
        signal = shutdown_signal() => {
            signal?;
            tracing::info!("shutdown signal received; stopping running instances");
            stop_running_instances(&state).await
        }
    }
}

fn stop_instances_on_shutdown() -> bool {
    let value = std::env::var(STOP_INSTANCES_ON_SHUTDOWN_ENV).ok();
    shutdown_flag_enabled(value.as_deref())
}

fn shutdown_flag_enabled(value: Option<&str>) -> bool {
    value.is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

async fn shutdown_signal() -> Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to register SIGTERM handler")?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.context("failed to register SIGINT handler")?,
        _ = terminate.recv() => {}
    }
    Ok(())
}

async fn stop_running_instances(state: &AppState) -> Result<()> {
    let valheim_instances = instance::list_all(&state.paths, &state.db)?;
    let valheim_names = valheim_instances
        .iter()
        .map(|instance| instance.state.name.clone())
        .collect::<Vec<_>>();
    let valheim_results = join_all(valheim_instances.iter().map(|instance| async {
        let name = &instance.state.name;
        if !dispatch::is_running_async(&state.paths, &state.db, GameId::Valheim, name).await? {
            return Ok(());
        }
        dispatch::stop(
            &state.paths,
            &state.db,
            GameId::Valheim,
            name,
            StopReason::HostShutdown,
        )
        .await
    }))
    .await;
    let mut failures = valheim_names
        .iter()
        .zip(valheim_results)
        .filter_map(|(name, result)| {
            result
                .err()
                .map(|error| format!("valheim/{name}: {error:#}"))
        })
        .collect::<Vec<_>>();

    for rust_instance in game_instances::list_rust(&state.db)? {
        if crate::game::rust::is_running(&state.paths, &state.db, &rust_instance)?
            && let Err(error) = crate::game::instances::stop_with(
                &state.paths,
                &state.db,
                GameId::Rust,
                rust_instance.name(),
                StopReason::HostShutdown,
            )
            .await
        {
            failures.push(format!("rust/{}: {error:#}", rust_instance.name()));
        }
    }
    for game in game_instances::generic_games() {
        for generic_instance in game_instances::list_generic(&state.db, game)? {
            if crate::game::instances::is_running(
                &state.paths,
                &state.db,
                game,
                generic_instance.name(),
            )? && let Err(error) = crate::game::instances::stop_with(
                &state.paths,
                &state.db,
                game,
                generic_instance.name(),
                StopReason::HostShutdown,
            )
            .await
            {
                failures.push(format!("{game}/{}: {error:#}", generic_instance.name()));
            }
        }
    }
    if !failures.is_empty() {
        bail!(
            "failed to stop one or more instances during shutdown: {}",
            failures.join("; ")
        );
    }

    tracing::info!(
        count = valheim_names.len(),
        "running Valheim instances stopped cleanly"
    );
    Ok(())
}

/// Best-effort discovery of the host's LAN IP, for display alongside the bind
/// address. Doesn't actually send any traffic: connecting a UDP socket only
/// picks the outbound interface/route, which `local_addr()` then reads back.
fn local_network_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip())
}

const TELEMETRY_INTERVAL: Duration = Duration::from_secs(3);
// How long to wait between automatic-restart attempts for the same
// instance. Without this, an instance that crashes immediately on every
// start (a broken mod, say) would get a fresh attempt on every telemetry
// tick — every few seconds, forever.
const AUTO_RESTART_COOLDOWN: chrono::Duration = chrono::Duration::seconds(60);

/// Background task keeping the dashboard's live view of the world warm:
/// refreshes `sysinfo` (its per-process CPU usage is a delta since the
/// previous refresh, so a `System` refreshed only on-demand would always
/// read 0%), then samples host and per-instance resource usage into
/// `state.runtime` so HTTP handlers and the live WebSocket feed just read a
/// cached snapshot instead of recomputing it per request. Also supervises
/// one player-tracking log tailer per currently-running instance, starting
/// and stopping them as instances start and stop, and restarts any instance
/// found dead that has opted into automatic crash recovery.
fn spawn_telemetry(state: AppState) {
    tokio::spawn(async move {
        let mut tailers: HashMap<String, AbortHandle> = HashMap::new();
        loop {
            let tick_state = state.clone();
            let tick = tokio::task::spawn_blocking(move || run_telemetry_tick(&tick_state))
                .await
                .unwrap_or_default();

            reconcile_log_tailers(&state, &mut tailers, &tick.running).await;
            for name in tick.crashed_with_auto_restart {
                attempt_auto_restart(&state, name).await;
            }
            for name in tick.crashed_rust_with_auto_restart {
                attempt_rust_auto_restart(&state, name).await;
            }
            for (game, name) in tick.crashed_generic_with_auto_restart {
                attempt_generic_auto_restart(&state, game, name).await;
            }

            tokio::time::sleep(TELEMETRY_INTERVAL).await;
        }
    });
}

async fn attempt_rust_auto_restart(state: &AppState, name: String) {
    tracing::warn!(instance = %name, game = "rust", "instance found dead; attempting automatic restart");
    let _transition = match state.runtime.begin_game_transition(
        GameId::Rust,
        &name,
        InstanceTransition::Starting,
    ) {
        Ok(transition) => transition,
        Err(error) => {
            tracing::debug!(instance = %name, game = "rust", %error, "automatic restart skipped");
            return;
        }
    };
    let instance = match game_instances::load_rust(&state.db, &name) {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(instance = %name, game = "rust", %error, "automatic restart skipped");
            return;
        }
    };
    match crate::game::instances::start(&state.paths, &state.db, GameId::Rust, instance.name())
        .await
    {
        Ok(_) => state.activity.record_for(
            GameId::Rust,
            ActivityKind::InstanceAutoRestarted,
            Some(name),
        ),
        Err(error) => {
            tracing::warn!(instance = %name, game = "rust", %error, "automatic restart failed")
        }
    }
}

async fn attempt_generic_auto_restart(state: &AppState, game: GameId, name: String) {
    tracing::warn!(instance = %name, %game, "instance found dead; attempting automatic restart");
    let _transition =
        match state
            .runtime
            .begin_game_transition(game, &name, InstanceTransition::Starting)
        {
            Ok(transition) => transition,
            Err(error) => {
                tracing::debug!(instance = %name, %game, %error, "automatic restart skipped");
                return;
            }
        };
    match crate::game::instances::start(&state.paths, &state.db, game, &name).await {
        Ok(_) => state
            .activity
            .record_for(game, ActivityKind::InstanceAutoRestarted, Some(name)),
        Err(error) => {
            tracing::warn!(instance = %name, %game, %error, "automatic restart failed")
        }
    }
}

/// Restarts an instance the telemetry tick found dead with automatic
/// restart enabled, recording a distinct activity kind from a deliberate
/// manual start so the feed doesn't conflate "an admin started it" with "it
/// crashed and came back on its own".
async fn attempt_auto_restart(state: &AppState, name: String) {
    tracing::warn!(instance = %name, "instance found dead; attempting automatic restart");
    let _transition = match state
        .runtime
        .begin_transition(&name, InstanceTransition::Starting)
    {
        Ok(transition) => transition,
        Err(error) => {
            tracing::debug!(instance = %name, %error, "automatic restart skipped");
            return;
        }
    };
    let started = crate::game::instances::start(&state.paths, &state.db, GameId::Valheim, &name)
        .await
        .map(|_| ());
    match started {
        Ok(()) => state
            .activity
            .record(ActivityKind::InstanceAutoRestarted, Some(name)),
        Err(e) => {
            tracing::warn!(instance = %name, error = %e, "automatic restart failed");
        }
    }
}

/// Id of the Valheim instance `name`, when it has one.
fn valheim_identity_id(state: &AppState, name: &str) -> Option<String> {
    crate::db::game_instances::identity(&state.db, GameId::Valheim, name)
        .ok()
        .flatten()
        .map(|identity| identity.id)
}

/// Whether a containerized instance found stopped should be restarted: an
/// operator never asked it to stop (`desired_running` is still set) and
/// automatic restart is enabled.
fn container_needs_restart(
    state: &AppState,
    game: GameId,
    instance_id: &str,
    name: &str,
    auto_restart: bool,
) -> bool {
    if !crate::db::container_instances::desired_running(&state.db, instance_id).unwrap_or(false) {
        return false;
    }
    if !auto_restart {
        // Nobody will bring it back, so stop claiming it should be running.
        let _ = crate::db::container_instances::set_desired_running(&state.db, instance_id, false);
        return false;
    }
    permits_scheduled_uptime(state, game, name)
        && state
            .runtime
            .should_attempt_game_auto_restart(game, name, AUTO_RESTART_COOLDOWN)
}

#[derive(Default)]
struct TelemetryTick {
    /// Names of instances found running this tick, so the caller can
    /// reconcile player tailers against them without a second pass over
    /// `instance::list_all`.
    running: Vec<String>,
    /// Names of instances found dead with automatic restart enabled (and
    /// past their cooldown), for the caller to restart — done outside this
    /// function since starting an instance is async and this runs on the
    /// blocking thread pool.
    crashed_with_auto_restart: Vec<String>,
    crashed_rust_with_auto_restart: Vec<String>,
    crashed_generic_with_auto_restart: Vec<(GameId, String)>,
}

fn run_telemetry_tick(state: &AppState) -> TelemetryTick {
    state
        .resources
        .lock()
        .expect("resources lock poisoned")
        .refresh_all();

    let host = compute_host_snapshot(state);
    state.runtime.push_host_sample(host);

    // Decided once per tick (not per series) so the host and every
    // instance's sample for this tick either all get persisted together or
    // none do.
    let persist_now = state.runtime.should_persist_now();
    let now = chrono::Utc::now();
    if persist_now {
        state
            .runtime
            .persist_sample(None, now, host.cpu_percent, host.memory_used_bytes);
    }

    let mut entries = Vec::new();
    let mut running_names = Vec::new();
    let mut crashed_with_auto_restart = Vec::new();
    let mut crashed_rust_with_auto_restart = Vec::new();
    let mut crashed_generic_with_auto_restart = Vec::new();
    if let Ok(instances) = instance::list_all(&state.paths, &state.db) {
        for inst in &instances {
            let Ok(snapshot) = compute_instance_snapshot(state, inst) else {
                continue;
            };
            let name = &inst.state.name;
            if persist_now && snapshot.running {
                state.runtime.persist_game_instance_sample(
                    GameId::Valheim,
                    name,
                    now,
                    snapshot.cpu_percent,
                    snapshot.memory_bytes,
                );
            }
            if state
                .runtime
                .push_game_instance_sample(GameId::Valheim, name, snapshot)
            {
                let kind = if snapshot.running {
                    ActivityKind::InstanceStarted
                } else {
                    ActivityKind::InstanceStopped
                };
                state
                    .activity
                    .record_for(GameId::Valheim, kind, Some(name.clone()));
            }
            if snapshot.running {
                running_names.push(name.clone());
            } else if let Some(id) = valheim_identity_id(state, name)
                && container_needs_restart(
                    state,
                    GameId::Valheim,
                    &id,
                    name,
                    inst.state.auto_restart,
                )
            {
                crashed_with_auto_restart.push(name.clone());
            }
            entries.push(InstanceResourceEntry {
                id: crate::db::game_instances::identity(&state.db, GameId::Valheim, name)
                    .ok()
                    .flatten()
                    .map(|identity| identity.id),
                game: GameId::Valheim,
                name: name.clone(),
                running: snapshot.running,
                ready: snapshot.ready,
                cpu_percent: snapshot.cpu_percent,
                memory_bytes: snapshot.memory_bytes,
                players: state.players.snapshot(name),
                last_saved_at: state.world_saves.get(name),
            });
        }
    }

    if let Ok(rust_instances) = game_instances::list_rust(&state.db) {
        for rust_instance in rust_instances {
            let name = rust_instance.name();
            let snapshot = match routes::games::rust_resource_snapshot(state, &rust_instance) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    // Unknown is not stopped: never act on a failed probe.
                    tracing::debug!(instance = %name, %error, "could not sample Rust instance");
                    continue;
                }
            };
            if persist_now && snapshot.running {
                state.runtime.persist_game_instance_sample(
                    GameId::Rust,
                    name,
                    now,
                    snapshot.cpu_percent,
                    snapshot.memory_bytes,
                );
            }
            if state
                .runtime
                .push_game_instance_sample(GameId::Rust, name, snapshot)
            {
                let kind = if snapshot.running {
                    ActivityKind::InstanceStarted
                } else {
                    ActivityKind::InstanceStopped
                };
                state
                    .activity
                    .record_for(GameId::Rust, kind, Some(name.to_string()));
            }
            if !snapshot.running
                && container_needs_restart(
                    state,
                    GameId::Rust,
                    &rust_instance.identity.id,
                    name,
                    rust_instance.config.auto_restart,
                )
            {
                crashed_rust_with_auto_restart.push(name.to_string());
            }
            entries.push(InstanceResourceEntry {
                id: Some(rust_instance.identity.id.clone()),
                game: GameId::Rust,
                name: name.to_string(),
                running: snapshot.running,
                ready: false,
                cpu_percent: snapshot.cpu_percent,
                memory_bytes: snapshot.memory_bytes,
                players: Vec::new(),
                last_saved_at: None,
            });
        }
    }

    for game in game_instances::generic_games() {
        let Ok(generic_instances) = game_instances::list_generic(&state.db, game) else {
            continue;
        };
        for generic_instance in generic_instances {
            let name = generic_instance.name();
            let snapshot = match routes::games::generic_resource_snapshot(state, &generic_instance)
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    // Unknown is not stopped: never act on a failed probe.
                    tracing::debug!(instance = %name, %error, "could not sample {game} instance");
                    continue;
                }
            };
            if persist_now && snapshot.running {
                state.runtime.persist_game_instance_sample(
                    game,
                    name,
                    now,
                    snapshot.cpu_percent,
                    snapshot.memory_bytes,
                );
            }
            if state
                .runtime
                .push_game_instance_sample(game, name, snapshot)
            {
                let kind = if snapshot.running {
                    ActivityKind::InstanceStarted
                } else {
                    ActivityKind::InstanceStopped
                };
                state
                    .activity
                    .record_for(game, kind, Some(name.to_string()));
            }
            if !snapshot.running
                && container_needs_restart(
                    state,
                    game,
                    &generic_instance.identity.id,
                    name,
                    generic_instance.config.auto_restart,
                )
            {
                crashed_generic_with_auto_restart.push((game, name.to_string()));
            }
            entries.push(InstanceResourceEntry {
                id: Some(generic_instance.identity.id.clone()),
                game,
                name: name.to_string(),
                running: snapshot.running,
                ready: false,
                cpu_percent: snapshot.cpu_percent,
                memory_bytes: snapshot.memory_bytes,
                players: Vec::new(),
                last_saved_at: None,
            });
        }
    }

    if persist_now {
        state.runtime.prune_old_samples();
    }

    state.runtime.broadcast_tick(ResourcesTick {
        host,
        instances: entries,
    });

    TelemetryTick {
        running: running_names,
        crashed_with_auto_restart,
        crashed_rust_with_auto_restart,
        crashed_generic_with_auto_restart,
    }
}

/// Crash recovery must honor an enabled operating-hours schedule too; without
/// this guard, a server that crashed outside its configured window could be
/// restarted by telemetry for one scheduler interval before being stopped.
fn permits_scheduled_uptime(state: &AppState, game: GameId, name: &str) -> bool {
    match crate::db::uptime_schedules::permits_running_now(&state.db, game, name) {
        Ok(permitted) => permitted,
        Err(error) => {
            tracing::warn!(%game, instance = %name, %error, "could not read uptime schedule; skipping automatic restart");
            false
        }
    }
}

/// Starts a live console feed (`web::log_tail`) for any newly-running
/// instance, and aborts + clears tracked players for any that stopped
/// running since the last tick.
async fn reconcile_log_tailers(
    state: &AppState,
    tailers: &mut HashMap<String, AbortHandle>,
    running: &[String],
) {
    let running_set: HashSet<&str> = running.iter().map(String::as_str).collect();

    tailers.retain(|name, handle| {
        if running_set.contains(name.as_str()) && !handle.is_finished() {
            true
        } else {
            handle.abort();
            if !running_set.contains(name.as_str()) {
                state.players.clear_instance(name);
                state.world_saves.clear_instance(name);
            }
            false
        }
    });

    for name in running {
        if tailers.contains_key(name) {
            continue;
        }
        let log_file = players::console_log_path(&state.paths.instance_dir(name));
        let sender = state.log_tail.sender_for(name);
        let handle = tokio::spawn(log_tail::tail_and_broadcast(
            name.clone(),
            log_file,
            sender,
            state.players.clone(),
            state.world_saves.clone(),
            state.activity.clone(),
        ))
        .abort_handle();
        tailers.insert(name.clone(), handle);
    }
}

#[cfg(test)]
mod tests {
    use super::{run_telemetry_tick, shutdown_flag_enabled, stop_running_instances};
    use crate::db::Db;
    use crate::game::GameId;
    use crate::instance::Instance;
    use crate::paths::Paths;
    use crate::web::state::AppState;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;

    #[test]
    fn shutdown_flag_is_enabled_for_one() {
        assert!(shutdown_flag_enabled(Some("1")));
    }

    #[test]
    fn shutdown_flag_is_enabled_for_case_insensitive_true() {
        assert!(shutdown_flag_enabled(Some("TRUE")));
    }

    #[test]
    fn shutdown_flag_is_disabled_when_missing() {
        assert!(!shutdown_flag_enabled(None));
    }

    #[tokio::test]
    async fn shutdown_stops_a_running_podman_valheim_instance() {
        let root = std::env::temp_dir().join(format!("odin-web-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config_dir: root.clone(),
            data_dir: root.clone(),
        };
        let socket = root.join("podman.sock");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                std::fs::remove_dir_all(root).ok();
                return;
            }
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let (status, body) = if request.starts_with("GET ") {
                    ("200 OK", r#"{"State":{"Running":true,"Status":"running"}}"#)
                } else {
                    ("204 No Content", "")
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                requests.push(request);
            }
            requests
        });
        std::fs::write(
            paths.config_file(),
            format!("[podman]\nsocket = \"{}\"\n", socket.display()),
        )
        .unwrap();
        let db = Arc::new(Db::open(&paths).unwrap());
        let instance = Instance::create(&paths, &db, "valheim").unwrap();
        let identity = crate::db::game_instances::ensure_valheim_identity(
            &db,
            &instance.state.name,
            instance.state.created_at,
        )
        .unwrap();
        crate::db::container_instances::set_desired_running(&db, &identity.id, true).unwrap();
        let state = AppState::new(paths.clone(), db.clone());

        stop_running_instances(&state).await.unwrap();

        // A shutdown is not an operator stop: the instance should come back.
        assert!(crate::db::container_instances::desired_running(&db, &identity.id).unwrap());

        assert!(
            server
                .join()
                .unwrap()
                .iter()
                .any(|request| request.starts_with("POST /v5.0.0/libpod/containers/"))
        );
        std::fs::remove_dir_all(root).ok();
    }

    /// A Podman whose every container is stopped, serving until the test ends.
    fn exited_podman(root: &std::path::Path) -> Option<()> {
        let socket = root.join("podman.sock");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut String::new())
                    .unwrap();
                let body = r#"{"State":{"Running":false,"Status":"exited"}}"#;
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        std::fs::write(
            root.join("config.toml"),
            format!("[podman]\nsocket = \"{}\"\n", socket.display()),
        )
        .unwrap();
        Some(())
    }

    fn container_state(label: &str) -> Option<(AppState, Arc<Db>, std::path::PathBuf)> {
        let root = std::env::temp_dir().join(format!("odin-web-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        exited_podman(&root)?;
        let paths = Paths {
            config_dir: root.clone(),
            data_dir: root.clone(),
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        Some((AppState::new(paths, db.clone()), db, root))
    }

    #[test]
    fn a_crashed_valheim_container_is_restarted_but_an_operator_stop_is_not() {
        let Some((state, db, root)) = container_state("crash-valheim") else {
            return;
        };
        let mut instance = Instance::create(&state.paths, &db, "valheim").unwrap();
        instance.state.auto_restart = true;
        instance.save(&db).unwrap();
        let identity = crate::db::game_instances::ensure_valheim_identity(
            &db,
            "valheim",
            instance.state.created_at,
        )
        .unwrap();
        let desired =
            |db: &Db| crate::db::container_instances::desired_running(db, &identity.id).unwrap();

        // Never started by an operator: nothing to recover.
        assert!(
            run_telemetry_tick(&state)
                .crashed_with_auto_restart
                .is_empty()
        );

        crate::db::container_instances::set_desired_running(&db, &identity.id, true).unwrap();
        assert_eq!(
            run_telemetry_tick(&state).crashed_with_auto_restart,
            ["valheim"]
        );
        // The cooldown spaces retries, and the intent survives for the next one.
        assert!(
            run_telemetry_tick(&state)
                .crashed_with_auto_restart
                .is_empty()
        );
        assert!(desired(&db));

        // An operator stop clears the intent, so a fresh cooldown still does nothing.
        crate::db::container_instances::set_desired_running(&db, &identity.id, false).unwrap();
        let fresh = AppState::new((*state.paths).clone(), db.clone());
        assert!(
            run_telemetry_tick(&fresh)
                .crashed_with_auto_restart
                .is_empty()
        );

        // With automatic restart off a dead instance is left alone and forgotten.
        instance.state.auto_restart = false;
        instance.save(&db).unwrap();
        crate::db::container_instances::set_desired_running(&db, &identity.id, true).unwrap();
        let fresh = AppState::new((*state.paths).clone(), db.clone());
        assert!(
            run_telemetry_tick(&fresh)
                .crashed_with_auto_restart
                .is_empty()
        );
        assert!(!desired(&db));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_crashed_rust_container_is_restarted_when_automatic_restart_is_on() {
        let Some((state, db, root)) = container_state("crash-rust") else {
            return;
        };
        let instance = crate::db::game_instances::create_rust(&state.paths, &db, "rusty").unwrap();
        let mut config = instance.config.clone();
        config.auto_restart = true;
        crate::db::game_instances::update_rust_config(&db, "rusty", &config).unwrap();
        crate::db::container_instances::set_desired_running(&db, &instance.identity.id, true)
            .unwrap();

        assert_eq!(
            run_telemetry_tick(&state).crashed_rust_with_auto_restart,
            ["rusty"]
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_crashed_7d2d_container_is_restarted_when_automatic_restart_is_on() {
        let Some((state, db, root)) = container_state("crash-7d2d") else {
            return;
        };
        let instance = crate::db::game_instances::create_generic(
            &state.paths,
            &db,
            GameId::SevenDaysToDie,
            "undead",
        )
        .unwrap();
        crate::db::game_instances::set_generic_auto_restart(
            &db,
            GameId::SevenDaysToDie,
            "undead",
            true,
        )
        .unwrap();
        crate::db::container_instances::set_desired_running(&db, &instance.identity.id, true)
            .unwrap();

        assert_eq!(
            run_telemetry_tick(&state).crashed_generic_with_auto_restart,
            [(GameId::SevenDaysToDie, "undead".to_string())]
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_stopped_7d2d_container_is_not_restarted_after_an_operator_stop() {
        let Some((state, db, root)) = container_state("stop-7d2d") else {
            return;
        };
        crate::db::game_instances::create_generic(
            &state.paths,
            &db,
            GameId::SevenDaysToDie,
            "undead",
        )
        .unwrap();
        crate::db::game_instances::set_generic_auto_restart(
            &db,
            GameId::SevenDaysToDie,
            "undead",
            true,
        )
        .unwrap();

        assert!(
            run_telemetry_tick(&state)
                .crashed_generic_with_auto_restart
                .is_empty()
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_crashed_palworld_container_is_restarted_when_automatic_restart_is_on() {
        let Some((state, db, root)) = container_state("crash-palworld") else {
            return;
        };
        let instance =
            crate::db::game_instances::create_generic(&state.paths, &db, GameId::Palworld, "pals")
                .unwrap();
        crate::db::game_instances::set_generic_auto_restart(&db, GameId::Palworld, "pals", true)
            .unwrap();
        crate::db::container_instances::set_desired_running(&db, &instance.identity.id, true)
            .unwrap();

        assert_eq!(
            run_telemetry_tick(&state).crashed_generic_with_auto_restart,
            [(GameId::Palworld, "pals".to_string())]
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_stopped_palworld_container_is_not_restarted_after_an_operator_stop() {
        let Some((state, db, root)) = container_state("stop-palworld") else {
            return;
        };
        crate::db::game_instances::create_generic(&state.paths, &db, GameId::Palworld, "pals")
            .unwrap();
        crate::db::game_instances::set_generic_auto_restart(&db, GameId::Palworld, "pals", true)
            .unwrap();

        assert!(
            run_telemetry_tick(&state)
                .crashed_generic_with_auto_restart
                .is_empty()
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_crashed_cs2_container_is_restarted_when_automatic_restart_is_on() {
        let Some((state, db, root)) = container_state("crash-cs2") else {
            return;
        };
        let instance =
            crate::db::game_instances::create_generic(&state.paths, &db, GameId::Cs2, "dust")
                .unwrap();
        crate::db::game_instances::set_generic_auto_restart(&db, GameId::Cs2, "dust", true)
            .unwrap();
        crate::db::container_instances::set_desired_running(&db, &instance.identity.id, true)
            .unwrap();

        assert_eq!(
            run_telemetry_tick(&state).crashed_generic_with_auto_restart,
            [(GameId::Cs2, "dust".to_string())]
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_stopped_cs2_container_is_not_restarted_after_an_operator_stop() {
        let Some((state, db, root)) = container_state("stop-cs2") else {
            return;
        };
        crate::db::game_instances::create_generic(&state.paths, &db, GameId::Cs2, "dust").unwrap();
        crate::db::game_instances::set_generic_auto_restart(&db, GameId::Cs2, "dust", true)
            .unwrap();

        assert!(
            run_telemetry_tick(&state)
                .crashed_generic_with_auto_restart
                .is_empty()
        );
        std::fs::remove_dir_all(root).ok();
    }
}
