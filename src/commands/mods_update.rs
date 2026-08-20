use anyhow::Result;

use crate::mods;
use crate::paths::Paths;

pub fn run(paths: &Paths, server_name: &str) -> Result<()> {
    mods::update(paths, server_name)
}
