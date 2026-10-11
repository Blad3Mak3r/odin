use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
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
    /// Podman's API socket. It is deliberately opt-in: access to this socket
    /// is equivalent to authority over the service user's containers.
    #[serde(default)]
    pub podman: Option<PodmanConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodmanConfig {
    pub socket: PathBuf,
    #[serde(default)]
    pub runtime_image: Option<String>,
}

impl PodmanConfig {
    pub fn runtime_image(&self) -> String {
        self.runtime_image
            .clone()
            .unwrap_or_else(|| format!("ghcr.io/blad3mak3r/odin-srv:{}", env!("CARGO_PKG_VERSION")))
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

    pub fn save(&self, paths: &Paths) -> Result<()> {
        if let Some(podman) = &self.podman
            && !podman.socket.is_absolute()
        {
            anyhow::bail!("podman.socket must be an absolute Unix socket path");
        }
        std::fs::create_dir_all(&paths.config_dir).with_context(|| {
            format!(
                "failed to create configuration directory {}",
                paths.config_dir.display()
            )
        })?;
        let config_file = paths.config_file();
        let temporary = config_file.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let content = toml::to_string_pretty(self).context("failed to encode configuration")?;
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)
                .with_context(|| format!("failed to create {}", temporary.display()))?;
            file.write_all(content.as_bytes())
                .with_context(|| format!("failed to write {}", temporary.display()))?;
            file.sync_all()
                .with_context(|| format!("failed to sync {}", temporary.display()))?;
            std::fs::rename(&temporary, &config_file).with_context(|| {
                format!(
                    "failed to atomically activate configuration {}",
                    config_file.display()
                )
            })
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        let root = std::env::temp_dir().join(format!("odin-config-{}", uuid::Uuid::new_v4()));
        Paths {
            config_dir: root.clone(),
            data_dir: root,
        }
    }

    #[test]
    fn save_persists_an_absolute_podman_socket() {
        let paths = paths();
        let config = GlobalConfig {
            data_dir: None,
            podman: Some(PodmanConfig {
                socket: "/run/podman/podman.sock".into(),
                runtime_image: None,
            }),
        };
        config.save(&paths).unwrap();
        let loaded = GlobalConfig::load(&paths).unwrap();
        assert_eq!(
            loaded.podman.unwrap().socket,
            PathBuf::from("/run/podman/podman.sock")
        );
        std::fs::remove_dir_all(paths.config_dir).unwrap();
    }

    #[test]
    fn save_rejects_a_relative_podman_socket() {
        let paths = paths();
        let config = GlobalConfig {
            data_dir: None,
            podman: Some(PodmanConfig {
                socket: "podman.sock".into(),
                runtime_image: None,
            }),
        };
        assert!(config.save(&paths).is_err());
    }

    #[test]
    fn save_removes_its_temporary_file_when_activation_fails() {
        let paths = paths();
        std::fs::create_dir_all(paths.config_file()).unwrap();

        assert!(GlobalConfig::default().save(&paths).is_err());
        assert!(std::fs::read_dir(&paths.config_dir).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")
        }));

        std::fs::remove_dir_all(paths.config_dir).unwrap();
    }
}
