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

macro_rules! valheim_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
        pub enum $name { $(#[serde(rename = $value)] $variant),+ }

        impl $name {
            pub const fn as_arg(self) -> &'static str {
                match self { $(Self::$variant => $value),+ }
            }
        }
    };
}

valheim_enum!(ValheimPreset {
    Normal => "normal", Casual => "casual", Easy => "easy", Hard => "hard",
    Hardcore => "hardcore", Immersive => "immersive", Hammer => "hammer"
});
impl ValheimPreset {
    pub fn from_arg(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "casual" => Some(Self::Casual),
            "easy" => Some(Self::Easy),
            "hard" => Some(Self::Hard),
            "hardcore" => Some(Self::Hardcore),
            "immersive" => Some(Self::Immersive),
            "hammer" => Some(Self::Hammer),
            _ => None,
        }
    }
}
valheim_enum!(ValheimCombatModifier {
    VeryEasy => "veryeasy", Easy => "easy", Hard => "hard", VeryHard => "veryhard"
});
valheim_enum!(ValheimDeathPenaltyModifier {
    Casual => "casual", VeryEasy => "veryeasy", Easy => "easy", Hard => "hard", Hardcore => "hardcore"
});
valheim_enum!(ValheimResourceModifier {
    MuchLess => "muchless", Less => "less", More => "more", MuchMore => "muchmore", Most => "most"
});
valheim_enum!(ValheimRaidModifier {
    None => "none", MuchLess => "muchless", Less => "less", More => "more", MuchMore => "muchmore"
});
valheim_enum!(ValheimPortalModifier {
    Casual => "casual", Hard => "hard", VeryHard => "veryhard"
});
valheim_enum!(ValheimSetKey {
    NoBuildCost => "nobuildcost", PlayerEvents => "playerevents",
    PassiveMobs => "passivemobs", NoMap => "nomap"
});

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValheimModifiers {
    pub combat: Option<ValheimCombatModifier>,
    pub death_penalty: Option<ValheimDeathPenaltyModifier>,
    pub resources: Option<ValheimResourceModifier>,
    pub raids: Option<ValheimRaidModifier>,
    pub portals: Option<ValheimPortalModifier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledMod {
    /// Thunderstore package id, `<namespace>-<name>`.
    pub mod_id: String,
    pub version: String,
    pub installed_at: DateTime<Utc>,
    /// Whether `BepInEx/plugins/<mod_id>` currently links to this exact
    /// version in the global store (loaded) or is absent (parked/disabled).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Pinned mods are skipped by bulk/per-instance update operations.
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    pub name: String,
    pub port: u16,
    pub world_name: String,
    pub password: Option<String>,
    pub public: bool,
    #[serde(default)]
    pub save_interval: Option<u32>,
    #[serde(default)]
    pub backups: Option<u16>,
    #[serde(default)]
    pub backup_short: Option<u32>,
    #[serde(default)]
    pub backup_long: Option<u32>,
    #[serde(default)]
    pub crossplay: bool,
    #[serde(default)]
    pub playfab_instance_id: Option<String>,
    #[serde(default)]
    pub preset: Option<ValheimPreset>,
    #[serde(default)]
    pub modifiers: ValheimModifiers,
    #[serde(default)]
    pub set_keys: Vec<ValheimSetKey>,
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
    pub bepinex_version: Option<String>,
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
            save_interval: None,
            backups: None,
            backup_short: None,
            backup_long: None,
            crossplay: false,
            playfab_instance_id: None,
            preset: None,
            modifiers: ValheimModifiers::default(),
            set_keys: Vec::new(),
            created_at: Utc::now(),
            last_started_at: None,
            last_stopped_at: None,
            pid: None,
            pid_started_at: None,
            bepinex_installed: false,
            bepinex_version: None,
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
            pinned: false,
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
