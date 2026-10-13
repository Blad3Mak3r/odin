//! Statically compiled game definitions.  A driver describes only facts that
//! are common to the host; game-specific state stays in its own module.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub mod ark_asa;
pub mod config_documents;
pub mod instances;
pub mod palworld;
pub mod ports;
pub mod rust;
pub mod seven_days_to_die;
pub mod valheim;

pub const SEVEN_DAYS_TEMPLATE_BASELINE_FILE: &str = ".odin-serverconfig-template.xml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameId {
    #[serde(rename = "ark-asa")]
    ArkSurvivalAscended,
    Valheim,
    Rust,
    Palworld,
    #[serde(rename = "7d2d")]
    SevenDaysToDie,
}

impl GameId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArkSurvivalAscended => "ark-asa",
            Self::Valheim => "valheim",
            Self::Rust => "rust",
            Self::Palworld => "palworld",
            Self::SevenDaysToDie => "7d2d",
        }
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for GameId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ark-asa" => Ok(Self::ArkSurvivalAscended),
            "valheim" => Ok(Self::Valheim),
            "rust" => Ok(Self::Rust),
            "palworld" => Ok(Self::Palworld),
            "7d2d" => Ok(Self::SevenDaysToDie),
            _ => Err(format!("unsupported game '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct GameCapabilities {
    pub backups: bool,
    pub players: bool,
    pub mods: bool,
    pub access_lists: bool,
    pub readiness: bool,
    pub shared_install: bool,
    /// An existing isolated instance can be moved onto the shared files
    /// without losing settings. Not implied by `shared_install`: ARK's shared
    /// configuration would hide an instance's own INI files.
    pub shared_install_migration: bool,
}

/// Runtime image required by a dedicated-server driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeImage {
    Base,
    Proton,
}

impl RuntimeImage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Proton => "proton",
        }
    }

    pub const fn all() -> [Self; 2] {
        [Self::Base, Self::Proton]
    }
}

/// Ports claimed by one default instance of a game. This belongs to the
/// compiled driver because it is part of the server's launch contract, not
/// an Odin-wide convention.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct GamePortRequirements {
    pub count: u16,
}

/// A statically compiled game module. Runtime-installed modules are
/// intentionally out of scope: every supported game ships with Odin.
pub trait GameDriver: Sync {
    fn id(&self) -> GameId;
    fn display_name(&self) -> &'static str;
    fn steam_app_id(&self) -> &'static str;
    fn server_binary(&self) -> &'static str;
    fn capabilities(&self) -> GameCapabilities;
    fn port_requirements(&self) -> GamePortRequirements;
    fn runtime_image(&self) -> RuntimeImage {
        RuntimeImage::Base
    }
    /// How long a graceful shutdown may take before the server is killed.
    /// It must cover the world save the game performs on exit.
    fn stop_timeout_secs(&self) -> u64 {
        30
    }
}

struct ValheimDriver;
struct ArkSurvivalAscendedDriver;
struct RustDriver;
struct PalworldDriver;
struct SevenDaysToDieDriver;

impl GameDriver for ValheimDriver {
    fn id(&self) -> GameId {
        GameId::Valheim
    }

    fn display_name(&self) -> &'static str {
        "Valheim"
    }

    fn steam_app_id(&self) -> &'static str {
        crate::steamcmd::VALHEIM_DEDICATED_SERVER_APP_ID
    }

    fn server_binary(&self) -> &'static str {
        "valheim_server.x86_64"
    }

    fn capabilities(&self) -> GameCapabilities {
        GameCapabilities {
            backups: true,
            players: true,
            mods: true,
            access_lists: true,
            readiness: true,
            shared_install: false,
            shared_install_migration: false,
        }
    }

    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 3 }
    }
}

impl GameDriver for RustDriver {
    fn id(&self) -> GameId {
        GameId::Rust
    }

    fn display_name(&self) -> &'static str {
        "Rust"
    }

    fn steam_app_id(&self) -> &'static str {
        rust::DEDICATED_SERVER_APP_ID
    }

    fn server_binary(&self) -> &'static str {
        "RustDedicated"
    }

    fn capabilities(&self) -> GameCapabilities {
        GameCapabilities {
            backups: true,
            players: false,
            mods: false,
            access_lists: true,
            readiness: false,
            shared_install: false,
            shared_install_migration: false,
        }
    }

    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 3 }
    }

    fn stop_timeout_secs(&self) -> u64 {
        // A large world takes a while to write on quit. LinuxGSM waits this
        // long for games that save on exit (its stop mode 4).
        120
    }
}

impl GameDriver for PalworldDriver {
    fn id(&self) -> GameId {
        GameId::Palworld
    }
    fn display_name(&self) -> &'static str {
        "Palworld"
    }
    fn steam_app_id(&self) -> &'static str {
        "2394010"
    }
    fn server_binary(&self) -> &'static str {
        "PalServer.sh"
    }
    fn capabilities(&self) -> GameCapabilities {
        GameCapabilities {
            backups: true,
            players: true,
            mods: false,
            access_lists: true,
            readiness: true,
            shared_install: false,
            shared_install_migration: false,
        }
    }
    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 1 }
    }
    fn stop_timeout_secs(&self) -> u64 {
        // The REST API's `shutdown` saves the world before the server exits.
        120
    }
}

impl GameDriver for SevenDaysToDieDriver {
    fn id(&self) -> GameId {
        GameId::SevenDaysToDie
    }
    fn display_name(&self) -> &'static str {
        "7 Days to Die"
    }
    fn steam_app_id(&self) -> &'static str {
        "294420"
    }
    fn server_binary(&self) -> &'static str {
        "7DaysToDieServer.x86_64"
    }
    fn capabilities(&self) -> GameCapabilities {
        GameCapabilities {
            backups: true,
            players: true,
            mods: true,
            access_lists: false,
            readiness: true,
            shared_install: true,
            shared_install_migration: true,
        }
    }
    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 3 }
    }
    fn stop_timeout_secs(&self) -> u64 {
        // The console's `shutdown` saves the world before the server exits.
        120
    }
}

impl GameDriver for ArkSurvivalAscendedDriver {
    fn id(&self) -> GameId {
        GameId::ArkSurvivalAscended
    }

    fn display_name(&self) -> &'static str {
        "ARK: Survival Ascended"
    }

    fn steam_app_id(&self) -> &'static str {
        "2430930"
    }

    fn server_binary(&self) -> &'static str {
        "ShooterGame/Binaries/Win64/ArkAscendedServer.exe"
    }

    fn capabilities(&self) -> GameCapabilities {
        GameCapabilities {
            backups: true,
            players: false,
            mods: true,
            access_lists: false,
            readiness: true,
            shared_install: true,
            shared_install_migration: false,
        }
    }

    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 3 }
    }

    fn runtime_image(&self) -> RuntimeImage {
        RuntimeImage::Proton
    }

    fn stop_timeout_secs(&self) -> u64 {
        180
    }
}

static VALHEIM: ValheimDriver = ValheimDriver;
static ARK_SURVIVAL_ASCENDED: ArkSurvivalAscendedDriver = ArkSurvivalAscendedDriver;
static RUST: RustDriver = RustDriver;
static PALWORLD: PalworldDriver = PalworldDriver;
static SEVEN_DAYS_TO_DIE: SevenDaysToDieDriver = SevenDaysToDieDriver;

pub fn driver(game: GameId) -> &'static dyn GameDriver {
    match game {
        GameId::ArkSurvivalAscended => &ARK_SURVIVAL_ASCENDED,
        GameId::Valheim => &VALHEIM,
        GameId::Rust => &RUST,
        GameId::Palworld => &PALWORLD,
        GameId::SevenDaysToDie => &SEVEN_DAYS_TO_DIE,
    }
}

pub fn drivers() -> [&'static dyn GameDriver; 5] {
    [
        &VALHEIM,
        &RUST,
        &PALWORLD,
        &SEVEN_DAYS_TO_DIE,
        &ARK_SURVIVAL_ASCENDED,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_gets_longer_than_the_default_to_save_its_world_on_stop() {
        assert_eq!(driver(GameId::Valheim).stop_timeout_secs(), 30);
        assert_eq!(driver(GameId::Rust).stop_timeout_secs(), 120);
    }

    #[test]
    fn rust_driver_has_the_dedicated_server_app_id() {
        assert_eq!(driver(GameId::Rust).steam_app_id(), "258550");
    }

    #[test]
    fn seven_days_to_die_driver_uses_the_short_public_id() {
        assert_eq!(GameId::SevenDaysToDie.as_str(), "7d2d");
        assert_eq!(
            serde_json::to_string(&GameId::SevenDaysToDie).unwrap(),
            "\"7d2d\""
        );
        assert_eq!(driver(GameId::SevenDaysToDie).steam_app_id(), "294420");
    }
}
