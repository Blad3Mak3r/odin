use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::game::RuntimeImage;
use crate::paths::Paths;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Override for the data dir; if None, `Paths::resolve` falls back to the
    /// XDG default (or the `ODIN_DATA_DIR` env var).
    #[serde(default)]
    pub data_dir: Option<PathBuf>,
    /// Podman's API socket. It is deliberately opt-in: access to this socket
    /// is equivalent to authority over the service user's containers.
    #[serde(default)]
    pub podman: Option<PodmanConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodmanConfig {
    pub socket: PathBuf,
    /// Deprecated alias for `base_image` retained for existing installations.
    #[serde(default)]
    pub runtime_image: Option<String>,
    #[serde(default)]
    pub base_image: Option<String>,
    #[serde(default)]
    pub proton_image: Option<String>,
    #[serde(default)]
    pub odin_binary: Option<PathBuf>,
}

impl PodmanConfig {
    pub fn image(&self, runtime: RuntimeImage) -> String {
        let base = || {
            self.base_image
                .as_ref()
                .or(self.runtime_image.as_ref())
                .cloned()
                .unwrap_or_else(|| "ghcr.io/blad3mak3r/odin-srv:base".into())
        };
        match runtime {
            RuntimeImage::Base => base(),
            RuntimeImage::Proton => self
                .proton_image
                .clone()
                .unwrap_or_else(|| "ghcr.io/blad3mak3r/odin-srv:proton".into()),
        }
    }

    pub fn odin_binary(&self) -> PathBuf {
        self.odin_binary
            .clone()
            .unwrap_or_else(|| PathBuf::from("/proc/self/exe"))
    }
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
