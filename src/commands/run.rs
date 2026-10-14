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

pub fn install_shared(
    game: crate::game::GameId,
    install_dir: &std::path::Path,
    steamcmd_dir: &std::path::Path,
    log_file: &std::path::Path,
) -> Result<()> {
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
    )?;
    if game == crate::game::GameId::ArkSurvivalAscended {
        crate::game::ark_asa::prepare_runtime(install_dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn shared_ark_install_sanitizes_the_depot_after_steamcmd_verifies_it() {
        let root =
            std::env::temp_dir().join(format!("odin-shared-ark-install-{}", uuid::Uuid::new_v4()));
        let install = root.join("shared");
        let steamcmd = root.join("steamcmd");
        let script = steamcmd.join("steamcmd.sh");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(
            &script,
            "#!/bin/sh\nfor arg in \"$@\"; do case \"$arg\" in \"+force_install_dir \"*) dir=\"${arg#+force_install_dir }\" ;; esac; done\nmkdir -p \"$dir/ShooterGame/Binaries/Win64\"\ntouch \"$dir/ShooterGame/Binaries/Win64/ArkAscendedServer.exe\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let dll = install.join("ShooterGame/Binaries/Win64/steamclient64.dll");
        std::fs::create_dir_all(dll.parent().unwrap()).unwrap();
        std::fs::write(&dll, "depot copy").unwrap();

        install_shared(
            crate::game::GameId::ArkSurvivalAscended,
            &install,
            &steamcmd,
            &root.join("logs/update.log"),
        )
        .unwrap();

        assert!(
            install
                .join("ShooterGame/Binaries/Win64/ArkAscendedServer.exe")
                .is_file()
        );
        assert!(!dll.exists());
        std::fs::remove_dir_all(root).ok();
    }
}
