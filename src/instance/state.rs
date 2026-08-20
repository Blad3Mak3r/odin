use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rand::Rng;
use rand::distributions::Alphanumeric;
use serde::{Deserialize, Serialize};

fn generate_password() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(12)
        .map(char::from)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledMod {
    /// Thunderstore package id, `<namespace>-<name>`.
    pub mod_id: String,
    pub version: String,
    pub installed_at: DateTime<Utc>,
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
    pub tmux_session: String,
    #[serde(default)]
    pub bepinex_installed: bool,
    #[serde(default)]
    pub installed_mods: Vec<InstalledMod>,
}

impl InstanceState {
    pub fn new(name: &str, port: u16) -> Self {
        let tmux_session = crate::paths::tmux_session_name(name);
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
            tmux_session,
            bepinex_installed: false,
            installed_mods: Vec::new(),
        }
    }

    pub fn load(state_file: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(state_file)
            .with_context(|| format!("failed to read state file {}", state_file.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse state file {}", state_file.display()))
    }

    /// Writes the state file atomically (write to a temp file, then rename over
    /// the target) so a process killed mid-write can't leave a corrupt state file.
    pub fn save(&self, state_file: &Path) -> Result<()> {
        if let Some(parent) = state_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create instance dir {}", parent.display()))?;
        }
        let raw =
            serde_json::to_string_pretty(self).context("failed to serialize instance state")?;
        let tmp_file = state_file.with_extension("json.tmp");
        std::fs::write(&tmp_file, raw)
            .with_context(|| format!("failed to write temp state file {}", tmp_file.display()))?;
        std::fs::rename(&tmp_file, state_file).with_context(|| {
            format!(
                "failed to rename {} to {}",
                tmp_file.display(),
                state_file.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("vm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state_file = dir.join("state.json");

        let mut original = InstanceState::new("my-server", 2456);
        original.installed_mods.push(InstalledMod {
            mod_id: "owner-mod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
        });

        original.save(&state_file).unwrap();
        let loaded = InstanceState::load(&state_file).unwrap();

        assert_eq!(loaded.name, original.name);
        assert_eq!(loaded.port, original.port);
        assert_eq!(loaded.installed_mods.len(), 1);
        assert_eq!(loaded.installed_mods[0].mod_id, "owner-mod");

        std::fs::remove_dir_all(&dir).ok();
    }
}
