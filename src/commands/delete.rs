use anyhow::{Context, Result, bail};

use crate::commands::confirm;
use crate::instance::{Instance, lifecycle};
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str, yes: bool, keep_backups: bool) -> Result<()> {
    let instance = Instance::load_existing(paths, server_name)?;

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

    if keep_backups {
        for entry in std::fs::read_dir(&instance.dir)
            .with_context(|| format!("failed to read instance dir {}", instance.dir.display()))?
        {
            let entry = entry?;
            if entry.file_name() == "backups" {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            }
            .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    } else {
        std::fs::remove_dir_all(&instance.dir)
            .with_context(|| format!("failed to remove instance dir {}", instance.dir.display()))?;
    }

    println!("deleted '{server_name}'");
    Ok(())
}
