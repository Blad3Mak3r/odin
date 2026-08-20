use anyhow::Result;

use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str, mod_id: &str) -> Result<()> {
    if mods::set_enabled(paths, server_name, mod_id, true)? {
        println!("enabled mod '{mod_id}' on '{server_name}'");
    } else {
        println!("mod '{mod_id}' on '{server_name}' is already enabled");
    }
    Ok(())
}
