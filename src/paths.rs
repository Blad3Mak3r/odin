use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;

/// Presence of `<SYSTEM_CONFIG_DIR>/config.toml` (shipped as a conffile by
/// the .deb/.rpm package) is what flips `Paths::resolve` into system mode.
pub const SYSTEM_CONFIG_DIR: &str = "/etc/odin";
pub const SYSTEM_DATA_DIR: &str = "/var/lib/odin";

/// Resolved filesystem layout for Odin, rooted at a data directory.
///
/// Precedence for the data dir: explicit override (config/env) > default
/// (system FHS paths if installed via package, otherwise XDG per-user
/// paths).
#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
}

/// Presence of `<SYSTEM_CONFIG_DIR>/config.toml` is what flips Odin into
/// system mode — see `Paths::resolve`'s doc comment. Also consulted by
/// `Paths::runtime_dir`, which needs the same distinction independently of
/// any already-resolved `Paths` value.
fn system_mode() -> bool {
    PathBuf::from(SYSTEM_CONFIG_DIR)
        .join("config.toml")
        .is_file()
}

impl Paths {
    pub fn resolve(data_dir_override: Option<PathBuf>) -> Result<Self> {
        let (config_dir, default_data_dir) = if system_mode() {
            (
                PathBuf::from(SYSTEM_CONFIG_DIR),
                PathBuf::from(SYSTEM_DATA_DIR),
            )
        } else {
            let project_dirs = ProjectDirs::from("", "", "odin")
                .context("could not determine home directory for XDG paths")?;
            (
                project_dirs.config_dir().to_path_buf(),
                project_dirs.data_dir().to_path_buf(),
            )
        };

        let data_dir = data_dir_override
            .or_else(|| std::env::var_os("ODIN_DATA_DIR").map(PathBuf::from))
            .unwrap_or(default_data_dir);

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

    /// A short, OS-managed directory for ephemeral runtime files — the
    /// `supervisor` module's per-instance Unix sockets. Deliberately *not*
    /// under `data_dir`: a Unix domain socket path is capped at ~108 bytes
    /// (`sockaddr_un::sun_path` on Linux), and `<data_dir>/servers/<name>/...`
    /// combined with a per-user XDG data dir and a long instance name can
    /// exceed that easily. System mode uses `/run/odin`; per-user mode
    /// follows the XDG Base Directory spec's `XDG_RUNTIME_DIR`, falling back
    /// to a `/tmp`-based path if it isn't set (e.g. no active login session).
    pub fn runtime_dir(&self) -> PathBuf {
        if system_mode() {
            return PathBuf::from("/run/odin");
        }
        ProjectDirs::from("", "", "odin")
            .and_then(|d| d.runtime_dir().map(std::path::Path::to_path_buf))
            .unwrap_or_else(|| std::env::temp_dir().join("odin-run"))
    }
}

pub fn instance_state_file(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join("state.json")
}

pub fn instance_saves_dir(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join("saves")
}

pub fn instance_backups_dir(instance_dir: &std::path::Path) -> PathBuf {
    instance_dir.join("backups")
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
