use anyhow::Result;

use crate::db::Db;
use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str, mod_id: &str) -> Result<()> {
    if mods::set_enabled(paths, db, server_name, mod_id, false)? {
        println!("disabled mod '{mod_id}' on '{server_name}'");
    } else {
        println!("mod '{mod_id}' on '{server_name}' is already disabled");
    }
    Ok(())
}
