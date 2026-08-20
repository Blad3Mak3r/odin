use anyhow::Result;

use crate::doctor;
use crate::paths::Paths;

pub fn run(paths: &Paths) -> Result<()> {
    doctor::run(paths)
}
