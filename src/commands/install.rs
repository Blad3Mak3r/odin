use anyhow::{Result, bail};

use crate::db::Db;
use crate::instance;
use crate::paths::Paths;
use crate::steamcmd::{SteamCmd, VALHEIM_DEDICATED_SERVER_APP_ID};

pub fn run(paths: &Paths, db: &Db) -> Result<()> {
    let running = instance::running_instance_names(paths, db)?;
    if !running.is_empty() {
        bail!(
            "refusing to install/update while instance(s) are running: {}; stop them first with `odin stop <name>`",
            running.join(", ")
        );
    }

    let steamcmd = SteamCmd::new(paths.steamcmd_dir());
    let install_dir = paths.shared_install_dir();
    let log_file = paths.data_dir.join("logs").join("steamcmd-install.log");

    steamcmd.update_app(
        VALHEIM_DEDICATED_SERVER_APP_ID,
        &install_dir,
        &log_file,
        |line| {
            println!("{line}");
        },
    )?;

    println!(
        "Valheim dedicated server installed/updated at {}",
        install_dir.display()
    );
    Ok(())
}
