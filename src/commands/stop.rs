use anyhow::Result;

use crate::db::Db;
use crate::paths::Paths;
use crate::podman::valheim;

pub fn run(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(valheim::stop(paths, db, server_name))?;
    println!("stopped '{server_name}'");
    Ok(())
}
