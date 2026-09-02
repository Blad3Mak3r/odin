//! Embedded web dashboard: a JSON API plus the built frontend, served from a
//! single async task started by `odin serve`. Everything else in the crate
//! is synchronous — this module (and `commands::serve`) is the only place
//! async/tokio is used.

mod backup_scheduler;
mod error;
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
pub mod supervisor;
mod update_monitor;
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
use routes::resources::{compute_host_snapshot, compute_instance_snapshot};
use runtime::InstanceTransition;
use runtime::{InstanceResourceEntry, ResourcesTick};
use state::AppState;

const STOP_INSTANCES_ON_SHUTDOWN_ENV: &str = "ODIN_STOP_INSTANCES_ON_SHUTDOWN";

pub async fn serve(paths: Paths, addr: SocketAddr) -> Result<()> {
    let db = Arc::new(Db::open(&paths).context("failed to open database")?);
    let state = AppState::new(paths, db);
    retention::run_once(&state);
    retention::spawn(state.clone());
    webhooks::spawn(state.clone());
    update_monitor::spawn(state.clone());
    spawn_telemetry(state.clone());
    backup_scheduler::spawn(state.clone());

    let router = router::build_router(state.clone());
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    tracing::info!(%addr, "odin dashboard listening");
    println!("Odin dashboard listening on http://{addr}");
    if let Some(ip) = local_network_ip() {
        println!("Network: http://{ip}:{}", addr.port());
    }

    let server = axum::serve(listener, router);
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
    let valheim_names = instance::running_instance_names(&state.paths, &state.db)?;
    let valheim_results = join_all(
        valheim_names
            .iter()
            .map(|name| instance::lifecycle::stop(&state.paths, &state.db, name)),
    )
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
        if rust_instance.is_running()
            && let Err(error) =
                crate::game::rust::stop(&state.paths, &state.db, &rust_instance).await
        {
            failures.push(format!("rust/{}: {error:#}", rust_instance.name()));
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

            tokio::time::sleep(TELEMETRY_INTERVAL).await;
        }
    });
}

async fn attempt_rust_auto_restart(state: &AppState, name: String) {
    tracing::warn!(instance = %name, game = "rust", "instance found dead; attempting automatic restart");
    let instance = match game_instances::load_rust(&state.db, &name) {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(instance = %name, game = "rust", %error, "automatic restart skipped");
            return;
        }
    };
    match crate::game::rust::start(&state.paths, &state.db, &instance).await {
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
    match instance::lifecycle::start(&state.paths, &state.db, &name).await {
        Ok(_) => state
            .activity
            .record(ActivityKind::InstanceAutoRestarted, Some(name)),
        Err(e) => {
            tracing::warn!(instance = %name, error = %e, "automatic restart failed");
        }
    }
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
    if let Ok(instances) = instance::list_all(&state.paths, &state.db) {
        for inst in &instances {
            let Ok(snapshot) = compute_instance_snapshot(state, inst) else {
                continue;
            };
            let name = &inst.state.name;
            if persist_now && snapshot.running {
                state.runtime.persist_sample(
                    Some(name),
                    now,
                    snapshot.cpu_percent,
                    snapshot.memory_bytes,
                );
            }
            if state.runtime.push_instance_sample(name, snapshot) {
                let kind = if snapshot.running {
                    ActivityKind::InstanceStarted
                } else {
                    ActivityKind::InstanceStopped
                };
                state.activity.record(kind, Some(name.clone()));
            }
            if snapshot.running {
                running_names.push(name.clone());
            } else if inst.state.pid.is_some() {
                // Persisted pid but not actually alive: the process died
                // without anyone observing it directly (crash, OOM, an
                // external `kill -9`). Clear the stale fingerprint so the
                // DB matches reality; `start`/`stop` already do this on
                // their own success path, so this is purely the safety net
                // for unwitnessed deaths.
                let _ = crate::db::instances::clear_pid(&state.db, name, chrono::Utc::now());
                // If a supervisor was involved, it's gone too (the ping
                // above already failed) without cleaning up after itself —
                // an `odin run` crash rather than a normal exit. Remove
                // whatever it left behind so a future start doesn't trip
                // over a stale socket/pidfile.
                let _ =
                    std::fs::remove_file(crate::supervisor::control_sock_path(&state.paths, name));
                let _ =
                    std::fs::remove_file(crate::supervisor::events_sock_path(&state.paths, name));
                let _ = std::fs::remove_file(crate::supervisor::pidfile_path(&state.paths, name));

                if inst.state.auto_restart
                    && state
                        .runtime
                        .should_attempt_auto_restart(name, AUTO_RESTART_COOLDOWN)
                {
                    crashed_with_auto_restart.push(name.clone());
                }
            }
            entries.push(InstanceResourceEntry {
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
            if rust_instance.is_running() || rust_instance.pid.is_none() {
                continue;
            }
            let name = rust_instance.name().to_string();
            let _ = game_instances::clear_rust_pid(&state.db, &name, chrono::Utc::now());
            if rust_instance.config.auto_restart
                && state
                    .runtime
                    .should_attempt_auto_restart(&format!("rust:{name}"), AUTO_RESTART_COOLDOWN)
            {
                crashed_rust_with_auto_restart.push(name);
            }
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
    }
}

/// Starts a live console feed (`web::log_tail`) for any newly-running
/// instance, and aborts + clears tracked players for any that stopped
/// running since the last tick. Prefers subscribing to the instance's
/// supervisor for pushed events (`web::supervisor::Supervisor::
/// try_bridge_events`); falls back to `web::log_tail`'s file-poller only
/// for an instance with no reachable supervisor (started by a pre-upgrade
/// binary, or whose supervisor has crashed).
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
        let handle = match state
            .supervisor
            .try_bridge_events(
                &state.paths,
                name,
                &state.log_tail,
                &state.players,
                &state.world_saves,
                &state.activity,
            )
            .await
        {
            Some(handle) => handle,
            None => {
                let log_file = players::console_log_path(&state.paths.instance_dir(name));
                let sender = state.log_tail.sender_for(name);
                tokio::spawn(log_tail::tail_and_broadcast(
                    name.clone(),
                    log_file,
                    sender,
                    state.players.clone(),
                    state.world_saves.clone(),
                    state.activity.clone(),
                ))
                .abort_handle()
            }
        };
        tailers.insert(name.clone(), handle);
    }
}

#[cfg(test)]
mod tests {
    use super::shutdown_flag_enabled;

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
}
