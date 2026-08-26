use anyhow::Result;

use crate::db::Db;
use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str, mod_id: &str) -> Result<()> {
    mods::add(paths, db, server_name, mod_id)?;
    println!("installed mod '{mod_id}' for '{server_name}'");
    Ok(())
}
