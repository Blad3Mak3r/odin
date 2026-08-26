use anyhow::Result;

use crate::db::Db;
use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str, mod_id: &str) -> Result<()> {
    mods::remove(paths, db, server_name, mod_id)?;
    println!("removed mod '{mod_id}' from '{server_name}'");
    Ok(())
}
