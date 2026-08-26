use anyhow::Result;

use crate::backup;
use crate::db::Db;
use crate::instance::Instance;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    let instance = Instance::load_existing(paths, db, server_name)?;
    let path = backup::create(&instance)?;
    println!("backed up '{server_name}' to {}", path.display());
    Ok(())
}
