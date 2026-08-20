use anyhow::Result;

use crate::instance::lifecycle;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str) -> Result<()> {
    lifecycle::stop(paths, server_name)?;
    println!("stopped '{server_name}'");
    Ok(())
}
