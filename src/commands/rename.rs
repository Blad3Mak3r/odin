use anyhow::Result;

use crate::db::Db;
use crate::instance::lifecycle;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, old_name: &str, new_name: &str) -> Result<()> {
    lifecycle::rename(paths, db, old_name, new_name)?;
    println!("renamed '{old_name}' to '{new_name}'");
    Ok(())
}
