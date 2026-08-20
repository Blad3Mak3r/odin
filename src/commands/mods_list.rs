use anyhow::Result;

use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str) -> Result<()> {
    let installed = mods::list(paths, server_name)?;

    if installed.is_empty() {
        println!("no mods installed on '{server_name}'");
        return Ok(());
    }

    println!("{:<40} {:<15} INSTALLED", "MOD", "VERSION");
    for m in installed {
        println!(
            "{:<40} {:<15} {}",
            m.mod_id,
            m.version,
            m.installed_at.format("%Y-%m-%d %H:%M")
        );
    }
    Ok(())
}
