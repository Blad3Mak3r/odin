use anyhow::Result;

use crate::instance::lifecycle;
use crate::paths::Paths;

pub fn run(paths: &Paths, old_name: &str, new_name: &str) -> Result<()> {
    lifecycle::rename(paths, old_name, new_name)?;
    println!("renamed '{old_name}' to '{new_name}'");
    Ok(())
}
