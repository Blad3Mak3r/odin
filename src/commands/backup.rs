use anyhow::Result;

use crate::backup;
use crate::db::Db;
use crate::instance::Instance;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    let instance = Instance::load_existing(paths, db, server_name)?;
    let entry = backup::create(&instance, db)?;
    println!("created backup '{}' for '{server_name}'", entry.id);
    Ok(())
}
