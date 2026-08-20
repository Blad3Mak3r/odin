use anyhow::Result;

use crate::instance::lifecycle;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str) -> Result<()> {
    let instance = lifecycle::start(paths, server_name)?;
    println!(
        "started '{server_name}' on port {} (password: {}); use `valheim console {server_name}` to attach",
        instance.state.port,
        instance.state.password.as_deref().unwrap_or("-")
    );
    Ok(())
}
