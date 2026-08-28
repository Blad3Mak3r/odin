use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rand::RngExt;
use rand::distr::Alphanumeric;
use serde::{Deserialize, Serialize};

fn generate_password() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(12)
        .map(char::from)
        .collect()
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledMod {
    /// Thunderstore package id, `<namespace>-<name>`.
    pub mod_id: String,
    pub version: String,
    pub installed_at: DateTime<Utc>,
    /// Whether `BepInEx/plugins/<mod_id>` currently exists as a symlink into
    /// the global mod store (loaded) or is absent (parked/disabled). The
    /// downloaded mod files themselves always live in the global store, one
    /// shared copy per mod_id — every instance referencing it sees whichever
    /// version was most recently fetched there (`version` below just
    /// records what this instance last saw, and can lag if another
    /// instance updates the shared copy).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    pub name: String,
    pub port: u16,
    pub world_name: String,
    pub password: Option<String>,
    pub public: bool,
    pub created_at: DateTime<Utc>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_stopped_at: Option<DateTime<Utc>>,
    /// OS process id of the running `valheim_server.x86_64`, if any. Always
    /// set together with `pid_started_at` and cleared together with it —
    /// never trust one without the other.
    #[serde(default)]
    pub pid: Option<u32>,
    /// The process's own kernel start time (seconds since epoch, from
    /// `sysinfo::Process::start_time()`) at the moment we recorded `pid`.
    /// Compared against the live value on every liveness check so a reused
    /// pid (after a host reboot, say) reads as "not running" rather than a
    /// false positive.
    #[serde(default)]
    pub pid_started_at: Option<i64>,
    #[serde(default)]
    pub bepinex_installed: bool,
    #[serde(default)]
    pub installed_mods: Vec<InstalledMod>,
    /// Whether the telemetry tick should restart this instance on its own
    /// if it finds the process dead without anyone having stopped it
    /// deliberately (crash, OOM, an external `kill -9`). Off by default —
    /// an admin opts an instance in from its Config tab.
    #[serde(default)]
    pub auto_restart: bool,
}

impl InstanceState {
    pub fn new(name: &str, port: u16) -> Self {
        Self {
            name: name.to_string(),
            port,
            world_name: name.to_string(),
            // Valheim requires a password of at least 5 characters; auto-generate
            // one since v1's CLI has no flag for setting it. Shown to the user
            // once, on creation, via `status`/`start` output.
            password: Some(generate_password()),
            public: true,
            created_at: Utc::now(),
            last_started_at: None,
            last_stopped_at: None,
            pid: None,
            pid_started_at: None,
            bepinex_installed: false,
            installed_mods: Vec::new(),
            auto_restart: false,
        }
    }

    /// Parses a `state.json` written by a pre-database version of Odin.
    /// State is now stored in SQLite (see `crate::db::instances`) — this
    /// only exists for the one-time bootstrap import of an existing
    /// installation.
    pub fn load_from_file(state_file: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(state_file)
            .with_context(|| format!("failed to read state file {}", state_file.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse state file {}", state_file.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_from_file_parses_a_legacy_state_json() {
        let dir = std::env::temp_dir().join(format!("vm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state_file = dir.join("state.json");

        let mut original = InstanceState::new("my-server", 2456);
        original.installed_mods.push(InstalledMod {
            mod_id: "owner-mod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
        });
        let raw = serde_json::to_string_pretty(&original).unwrap();
        std::fs::write(&state_file, raw).unwrap();

        let loaded = InstanceState::load_from_file(&state_file).unwrap();

        assert_eq!(loaded.name, original.name);
        assert_eq!(loaded.port, original.port);
        assert_eq!(loaded.installed_mods.len(), 1);
        assert_eq!(loaded.installed_mods[0].mod_id, "owner-mod");

        std::fs::remove_dir_all(&dir).ok();
    }
}
