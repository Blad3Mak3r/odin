use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths::Paths;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Override for the data dir; if None, `Paths::resolve` falls back to the
    /// XDG default (or the `ODIN_DATA_DIR` env var).
    #[serde(default)]
    pub data_dir: Option<PathBuf>,
}

impl GlobalConfig {
    pub fn load(paths: &Paths) -> Result<Self> {
        let config_file = paths.config_file();
        if !config_file.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&config_file)
            .with_context(|| format!("failed to read config file {}", config_file.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse config file {}", config_file.display()))
    }
}
