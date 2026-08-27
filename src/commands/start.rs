use anyhow::Result;

use crate::db::Db;
use crate::instance::lifecycle;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    let instance = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(lifecycle::start(paths, db, server_name))?;
    println!(
        "started '{server_name}' on port {} (password: {}); use `odin logs --follow {server_name}` to watch the console",
        instance.state.port,
        instance.state.password.as_deref().unwrap_or("-")
    );
    Ok(())
}
