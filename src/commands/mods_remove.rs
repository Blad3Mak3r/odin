use anyhow::Result;

use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str, mod_id: &str) -> Result<()> {
    mods::remove(paths, server_name, mod_id)?;
    println!("removed mod '{mod_id}' from '{server_name}'");
    Ok(())
}
