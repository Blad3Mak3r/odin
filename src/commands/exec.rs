use anyhow::{Result, bail};

use crate::db::Db;
use crate::instance::{Instance, InstanceError, lifecycle};
use crate::paths::Paths;
use crate::tmux;

pub fn run(paths: &Paths, db: &Db, server_name: &str, command: &str) -> Result<()> {
    let instance = Instance::load_existing(paths, db, server_name)?;
    if !lifecycle::is_running(&instance)? {
        bail!(InstanceError::NotRunning(server_name.to_string()));
    }
    tmux::send_keys_line(&instance.state.tmux_session, command)?;
    println!("sent to '{server_name}': {command}");
    Ok(())
}
