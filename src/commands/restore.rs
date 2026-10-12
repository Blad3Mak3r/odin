use anyhow::{Result, bail};

use crate::backup;
use crate::db::Db;
use crate::instance::Instance;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str, backup_id: Option<&str>) -> Result<()> {
    let instance = Instance::load_existing(paths, db, server_name)?;

    let Some(backup_id) = backup_id else {
        let backups = backup::list(db, &instance.state.name)?;
        if backups.is_empty() {
            println!("no backups found for '{server_name}'; run `odin backup {server_name}` first");
        } else {
            println!("{:<20} {:<20} SIZE", "ID", "CREATED");
            for b in backups {
                println!(
                    "{:<20} {:<20} {} bytes",
                    b.id,
                    b.created_at.format("%Y-%m-%d %H:%M:%S"),
                    b.size_bytes
                );
            }
        }
        return Ok(());
    };

    if crate::podman::valheim::is_running(paths, db, &instance)? {
        bail!("'{server_name}' is running; stop it first with `odin stop {server_name}`");
    }

    backup::restore(paths, &instance, db, backup_id)?;
    println!(
        "restored '{server_name}' from backup '{backup_id}' (previous saves were backed up first)"
    );
    Ok(())
}
