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

pub fn install_ark_shared(
    install_dir: &std::path::Path,
    steamcmd_dir: &std::path::Path,
    log_file: &std::path::Path,
) -> Result<()> {
    let game = crate::game::GameId::ArkSurvivalAscended;
    // Use the SteamCMD shipped in the image instead of downloading it again.
    crate::podman::runtime::seed_steamcmd(steamcmd_dir)?;
    let expected = install_dir.join(crate::game::driver(game).server_binary());
    crate::steamcmd::SteamCmd::new(steamcmd_dir.to_path_buf()).update_app_expect_file(
        crate::game::driver(game).steam_app_id(),
        install_dir,
        log_file,
        expected,
        // The container's stdout is streamed to the dashboard job log.
        |line| println!("{line}"),
    )
}
