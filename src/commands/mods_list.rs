use anyhow::Result;

use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str) -> Result<()> {
    let installed = mods::list(paths, server_name)?;

    if installed.is_empty() {
        println!("no mods installed on '{server_name}'");
        return Ok(());
    }

    println!("{:<40} {:<15} {:<8} INSTALLED", "MOD", "VERSION", "ENABLED");
    for m in installed {
        println!(
            "{:<40} {:<15} {:<8} {}",
            m.mod_id,
            m.version,
            if m.enabled { "yes" } else { "no" },
            m.installed_at.format("%Y-%m-%d %H:%M")
        );
    }
    Ok(())
}
