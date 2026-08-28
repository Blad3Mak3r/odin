//! Embedded web dashboard: a JSON API plus the built frontend, served from a
//! single async task started by `odin serve`. Everything else in the crate
//! is synchronous — this module (and `commands::serve`) is the only place
//! async/tokio is used.

mod backup_scheduler;
mod error;
pub mod jobs;
mod log_tail;
mod players;
mod router;
pub mod routes;
mod runtime;
mod sse;
mod state;
mod static_files;
pub mod supervisor;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::task::AbortHandle;

use crate::activity::ActivityKind;
use crate::db::Db;
use crate::instance;
use crate::paths::Paths;
use routes::resources::{compute_host_snapshot, compute_instance_snapshot};
use runtime::{InstanceResourceEntry, ResourcesTick};
use state::AppState;

pub async fn serve(paths: Paths, addr: SocketAddr) -> Result<()> {
    let db = Arc::new(Db::open(&paths).context("failed to open database")?);
    let state = AppState::new(paths, db);
    spawn_telemetry(state.clone());
    backup_scheduler::spawn(state.clone());

    let router = router::build_router(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    tracing::info!(%addr, "odin dashboard listening");
    println!("Odin dashboard listening on http://{addr}");
    if let Some(ip) = local_network_ip() {
        println!("Network: http://{ip}:{}", addr.port());
    }
    axum::serve(listener, router)
        .await
        .context("web server error")
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

/// Background task keeping the dashboard's live view of the world warm:
/// refreshes `sysinfo` (its per-process CPU usage is a delta since the
/// previous refresh, so a `System` refreshed only on-demand would always
/// read 0%), then samples host and per-instance resource usage into
/// `state.runtime` so HTTP handlers and the live WebSocket feed just read a
/// cached snapshot instead of recomputing it per request. Also supervises
/// one player-tracking log tailer per currently-running instance, starting
/// and stopping them as instances start and stop.
fn spawn_telemetry(state: AppState) {
    tokio::spawn(async move {
        let mut tailers: HashMap<String, AbortHandle> = HashMap::new();
        loop {
            let tick_state = state.clone();
            let running = tokio::task::spawn_blocking(move || run_telemetry_tick(&tick_state))
                .await
                .unwrap_or_default();

            reconcile_log_tailers(&state, &mut tailers, &running).await;

            tokio::time::sleep(TELEMETRY_INTERVAL).await;
        }
    });
}

/// One tick of resource sampling; returns the names of instances found
/// running this tick, so the caller can reconcile player tailers against
/// them without a second pass over `instance::list_all`.
fn run_telemetry_tick(state: &AppState) -> Vec<String> {
    state
        .resources
        .lock()
        .expect("resources lock poisoned")
        .refresh_all();

    let host = compute_host_snapshot(state);
    state.runtime.push_host_sample(host);

    let mut entries = Vec::new();
    let mut running_names = Vec::new();
    if let Ok(instances) = instance::list_all(&state.paths, &state.db) {
        for inst in &instances {
            let Ok(snapshot) = compute_instance_snapshot(state, inst) else {
                continue;
            };
            let name = &inst.state.name;
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
            }
            entries.push(InstanceResourceEntry {
                name: name.clone(),
                running: snapshot.running,
                cpu_percent: snapshot.cpu_percent,
                memory_bytes: snapshot.memory_bytes,
                players: state.players.snapshot(name),
            });
        }
    }

    state.runtime.broadcast_tick(ResourcesTick {
        host,
        instances: entries,
    });

    running_names
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
        if running_set.contains(name.as_str()) {
            true
        } else {
            handle.abort();
            state.players.clear_instance(name);
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
                    state.activity.clone(),
                ))
                .abort_handle()
            }
        };
        tailers.insert(name.clone(), handle);
    }
}
