use anyhow::{Context, Result};

use crate::paths::Paths;

/// Hidden: the per-instance supervisor process (odin's conmon equivalent).
/// Long-lived, but — unlike `commands::serve`, which needs real concurrency
/// for HTTP request handling — its own workload is tiny: one Valheim
/// child, two Unix sockets, and a handful of connections at a time. A
/// `new_multi_thread` runtime defaults to one worker thread per CPU core,
/// which is one whole thread pool per *instance* for essentially no
/// concurrent work; `new_current_thread` runs everything (including the
/// `tokio::spawn`ed connection/log-poller tasks) cooperatively on this one
/// thread instead, exactly like `commands::start`/`stop`/`restart` already
/// do for their own lightweight async needs.
pub fn run(paths: &Paths, instance_name: &str) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to start async runtime")?
        .block_on(crate::supervisor::server::run_instance(
            paths.clone(),
            instance_name,
        ))
}
