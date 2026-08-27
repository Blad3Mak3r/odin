use anyhow::{Context, Result};

use crate::paths::Paths;

/// Hidden: the per-instance supervisor process (odin's conmon equivalent).
/// Owns its own multi-threaded tokio runtime, like `commands::serve` — this
/// is a genuinely long-lived process, not a one-shot CLI invocation.
pub fn run(paths: &Paths, instance_name: &str) -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start async runtime")?
        .block_on(crate::supervisor::server::run_instance(
            paths.clone(),
            instance_name,
        ))
}
