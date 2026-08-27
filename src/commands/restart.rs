use anyhow::Result;

use crate::db::Db;
use crate::instance::lifecycle;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    let instance = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(lifecycle::restart(paths, db, server_name))?;
    println!(
        "restarted '{server_name}' on port {} (password: {})",
        instance.state.port,
        instance.state.password.as_deref().unwrap_or("-")
    );
    Ok(())
}
