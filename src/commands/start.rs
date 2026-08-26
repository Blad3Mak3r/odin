use anyhow::Result;

use crate::db::Db;
use crate::instance::lifecycle;
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    let (instance, child) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(lifecycle::start(paths, db, server_name))?;
    // No supervisor outside `odin serve` to hand this to — dropping it is
    // safe and intentional (tokio's `kill_on_drop` defaults to false), and
    // leaves the process adoptable by whichever `odin serve` next
    // reconciles, exactly as it used to sit detached in a tmux session
    // nobody was attached to.
    drop(child);
    println!(
        "started '{server_name}' on port {} (password: {}); use `odin logs --follow {server_name}` to watch the console",
        instance.state.port,
        instance.state.password.as_deref().unwrap_or("-")
    );
    Ok(())
}
