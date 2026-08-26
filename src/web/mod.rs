//! Embedded web dashboard: a JSON API plus the built frontend, served from a
//! single async task started by `odin serve`. Everything else in the crate
//! is synchronous — this module (and `commands::serve`) is the only place
//! async/tokio is used.

mod error;
pub mod jobs;
mod router;
pub mod routes;
mod runtime;
mod state;
mod static_files;
mod ws;

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::instance;
use crate::paths::Paths;
use routes::resources::{compute_host_snapshot, compute_instance_snapshot};
use state::AppState;

pub async fn serve(paths: Paths, addr: SocketAddr) -> Result<()> {
    let state = AppState::new(paths);
    spawn_telemetry(state.clone());

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
/// cached snapshot instead of recomputing it per request.
fn spawn_telemetry(state: AppState) {
    tokio::spawn(async move {
        loop {
            let tick_state = state.clone();
            let _ = tokio::task::spawn_blocking(move || run_telemetry_tick(&tick_state)).await;
            tokio::time::sleep(TELEMETRY_INTERVAL).await;
        }
    });
}

fn run_telemetry_tick(state: &AppState) {
    state
        .resources
        .lock()
        .expect("resources lock poisoned")
        .refresh_all();

    state.runtime.push_host_sample(compute_host_snapshot(state));

    let Ok(instances) = instance::list_all(&state.paths) else {
        return;
    };
    for inst in &instances {
        if let Ok(snapshot) = compute_instance_snapshot(state, inst) {
            state
                .runtime
                .push_instance_sample(&inst.state.name, snapshot);
        }
    }
}
