use anyhow::{Context, Result};

pub fn run_container(spec_path: &std::path::Path, instance_id: String) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to start async runtime")?;
    let result = runtime.block_on(crate::podman::runtime::run(spec_path, &instance_id));
    // A shutdown request can arrive while SteamCMD is still downloading on a
    // blocking thread; do not wait for it, the container is going away.
    runtime.shutdown_background();
    result
}
