use anyhow::{Result, bail};

use crate::instance;
use crate::paths::Paths;
use crate::steamcmd::{SteamCmd, VALHEIM_DEDICATED_SERVER_APP_ID};

pub fn run(paths: &Paths) -> Result<()> {
    let running = instance::running_instance_names(paths)?;
    if !running.is_empty() {
        bail!(
            "refusing to install/update while instance(s) are running: {}; stop them first with `valheim stop <name>`",
            running.join(", ")
        );
    }

    let steamcmd = SteamCmd::new(paths.steamcmd_dir());
    let install_dir = paths.shared_install_dir();
    let log_file = paths.data_dir.join("logs").join("steamcmd-install.log");

    steamcmd.update_app(VALHEIM_DEDICATED_SERVER_APP_ID, &install_dir, &log_file)?;

    println!(
        "Valheim dedicated server installed/updated at {}",
        install_dir.display()
    );
    Ok(())
}
