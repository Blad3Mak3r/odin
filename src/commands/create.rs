use anyhow::Result;

use crate::instance::Instance;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str) -> Result<()> {
    let instance = Instance::create(paths, server_name)?;
    println!(
        "created '{server_name}' on port {} (password: {}); use `odin start {server_name}` to launch it",
        instance.state.port,
        instance.state.password.as_deref().unwrap_or("-")
    );
    Ok(())
}
