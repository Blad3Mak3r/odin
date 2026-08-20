use anyhow::Result;

use crate::instance::lifecycle;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str) -> Result<()> {
    let instance = lifecycle::restart(paths, server_name)?;
    println!(
        "restarted '{server_name}' on port {} (password: {})",
        instance.state.port,
        instance.state.password.as_deref().unwrap_or("-")
    );
    Ok(())
}
