use anyhow::Result;

use crate::db::Db;
use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    mods::update(paths, db, server_name)
}
