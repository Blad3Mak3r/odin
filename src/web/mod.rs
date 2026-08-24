//! Embedded web dashboard: a JSON API plus the built frontend, served from a
//! single async task started by `odin serve`. Everything else in the crate
//! is synchronous — this module (and `commands::serve`) is the only place
//! async/tokio is used.

mod error;
pub mod jobs;
mod router;
pub mod routes;
mod state;
mod static_files;
mod ws;

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::paths::Paths;
use state::AppState;

pub async fn serve(paths: Paths, addr: SocketAddr) -> Result<()> {
    let state = AppState::new(paths);
    spawn_resource_refresh(state.clone());

    let router = router::build_router(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    tracing::info!(%addr, "odin dashboard listening");
    println!("Odin dashboard listening on http://{addr}");
    axum::serve(listener, router)
        .await
        .context("web server error")
}

/// `sysinfo`'s per-process CPU usage is a delta since the previous refresh,
/// so a `System` refreshed only on-demand would always read 0% — this keeps
/// `AppState.resources` warm in the background instead.
fn spawn_resource_refresh(state: AppState) {
    tokio::spawn(async move {
        loop {
            let system = state.resources.clone();
            let _ = tokio::task::spawn_blocking(move || {
                system
                    .lock()
                    .expect("resources lock poisoned")
                    .refresh_all();
            })
            .await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}
