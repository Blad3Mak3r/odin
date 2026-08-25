use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

/// Resolved filesystem layout for Odin, rooted at a data directory.
///
/// Precedence for the data dir: explicit override (config/env) > XDG default
/// (`~/.local/share/odin`).
#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

impl Paths {
    pub fn resolve(data_dir_override: Option<PathBuf>) -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "odin")
            .context("could not determine home directory for XDG paths")?;

        let data_dir = data_dir_override
            .or_else(|| std::env::var_os("ODIN_DATA_DIR").map(PathBuf::from))
            .unwrap_or_else(|| project_dirs.data_dir().to_path_buf());

        let config_dir = project_dirs.config_dir().to_path_buf();

        Ok(Self {
            data_dir,
            config_dir,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn steamcmd_dir(&self) -> PathBuf {
        self.data_dir.join("steamcmd")
    }

    pub fn shared_install_dir(&self) -> PathBuf {
        self.data_dir.join("install").join("valheim")
    }

    pub fn servers_dir(&self) -> PathBuf {
        self.data_dir.join("servers")
    }

    pub fn instance_dir(&self, name: &str) -> PathBuf {
        self.servers_dir().join(name)
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }

    pub fn thunderstore_index_cache(&self) -> PathBuf {
        self.cache_dir().join("thunderstore-index.json")
    }

    /// Root of the global mod store, shared across every instance.
    pub fn mods_dir(&self) -> PathBuf {
        self.data_dir.join("mods")
    }

    /// A mod's payload in the global store — one shared copy per mod_id,
    /// whichever version was most recently fetched. Instances symlink into
    /// this rather than each keeping their own copy.
    pub fn mod_dir(&self, mod_id: &str) -> PathBuf {
        self.mods_dir().join(mod_id)
    }
}

pub fn instance_state_file(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join("state.json")
}

pub fn instance_saves_dir(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join("saves")
}

pub fn instance_logs_dir(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join("logs")
}

pub fn instance_bepinex_dir(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join("BepInEx")
}

pub fn instance_bepinex_config_dir(instance_dir: &std::path::Path) -> PathBuf {
    instance_bepinex_dir(instance_dir).join("config")
}

pub fn instance_server_symlink(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join("server")
}

pub fn tmux_session_name(instance_name: &str) -> String {
    format!("odin-{instance_name}")
}
