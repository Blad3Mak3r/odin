use anyhow::{Result, bail};

use crate::commands::confirm;
use crate::db::Db;
use crate::instance::{Instance, lifecycle};
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str, yes: bool, keep_backups: bool) -> Result<()> {
    let instance = Instance::load_existing(paths, db, server_name)?;

    if lifecycle::is_running(&instance)? {
        bail!("'{server_name}' is running; stop it first with `odin stop {server_name}`");
    }

    if !yes {
        let scope = if keep_backups {
            "world saves, config, and mods (backups will be kept)"
        } else {
            "world saves, config, mods, and backups"
        };
        let prompt = format!("this will permanently delete '{server_name}' ({scope}). Continue?");
        if !confirm(&prompt)? {
            println!("aborted");
            return Ok(());
        }
    }

    lifecycle::delete(db, &instance, keep_backups)?;

    println!("deleted '{server_name}'");
    Ok(())
}
