use anyhow::Result;

use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str, mod_id: &str) -> Result<()> {
    mods::add(paths, server_name, mod_id)?;
    println!("installed mod '{mod_id}' for '{server_name}'");
    Ok(())
}
