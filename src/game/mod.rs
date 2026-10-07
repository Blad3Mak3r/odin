//! Statically compiled game definitions.  A driver describes only facts that
//! are common to the host; game-specific state stays in its own module.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub mod config_documents;
pub mod generic;
pub mod instances;
pub mod palworld;
pub mod ports;
pub mod proton_ge;
pub mod rust;
pub mod seven_days_to_die;
pub mod update;
pub mod valheim;
pub mod vrising;

pub const SEVEN_DAYS_TEMPLATE_BASELINE_FILE: &str = ".odin-serverconfig-template.xml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameId {
    Valheim,
    Rust,
    VRising,
    Palworld,
    RunescapeDragonwilds,
    #[serde(rename = "7d2d")]
    SevenDaysToDie,
}

impl GameId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valheim => "valheim",
            Self::Rust => "rust",
            Self::VRising => "vrising",
            Self::Palworld => "palworld",
            Self::RunescapeDragonwilds => "runescape-dragonwilds",
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
            "valheim" => Ok(Self::Valheim),
            "rust" => Ok(Self::Rust),
            "vrising" => Ok(Self::VRising),
            "palworld" => Ok(Self::Palworld),
            "runescape-dragonwilds" => Ok(Self::RunescapeDragonwilds),
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
}

/// Ports claimed by one default instance of a game. This belongs to the
/// compiled driver because it is part of the server's launch contract, not
/// an Odin-wide convention.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct GamePortRequirements {
    pub count: u16,
}

/// A statically compiled game module. Runtime-installed modules are
/// intentionally out of scope: both supported games ship with Odin.
pub trait GameDriver: Sync {
    fn id(&self) -> GameId;
    fn display_name(&self) -> &'static str;
    fn steam_app_id(&self) -> &'static str;
    fn server_binary(&self) -> &'static str;
    fn capabilities(&self) -> GameCapabilities;
    fn port_requirements(&self) -> GamePortRequirements;
}

struct ValheimDriver;
struct RustDriver;
struct VRisingDriver;
struct PalworldDriver;
struct RunescapeDragonwildsDriver;
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
        }
    }

    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 3 }
    }
}

impl GameDriver for VRisingDriver {
    fn id(&self) -> GameId {
        GameId::VRising
    }
    fn display_name(&self) -> &'static str {
        "V Rising"
    }
    fn steam_app_id(&self) -> &'static str {
        "1829350"
    }
    fn server_binary(&self) -> &'static str {
        "VRisingServer.exe"
    }
    fn capabilities(&self) -> GameCapabilities {
        GameCapabilities {
            backups: true,
            players: false,
            mods: false,
            access_lists: true,
            readiness: false,
        }
    }
    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 2 }
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
        }
    }
    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 1 }
    }
}

impl GameDriver for RunescapeDragonwildsDriver {
    fn id(&self) -> GameId {
        GameId::RunescapeDragonwilds
    }
    fn display_name(&self) -> &'static str {
        "RuneScape: Dragonwilds"
    }
    fn steam_app_id(&self) -> &'static str {
        "4019830"
    }
    fn server_binary(&self) -> &'static str {
        "RSDragonwildsServer.sh"
    }
    fn capabilities(&self) -> GameCapabilities {
        GameCapabilities {
            backups: true,
            players: false,
            mods: false,
            access_lists: true,
            readiness: false,
        }
    }
    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 2 }
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
        }
    }
    fn port_requirements(&self) -> GamePortRequirements {
        GamePortRequirements { count: 3 }
    }
}

static VALHEIM: ValheimDriver = ValheimDriver;
static RUST: RustDriver = RustDriver;
static V_RISING: VRisingDriver = VRisingDriver;
static PALWORLD: PalworldDriver = PalworldDriver;
static RUNESCAPE_DRAGONWILDS: RunescapeDragonwildsDriver = RunescapeDragonwildsDriver;
static SEVEN_DAYS_TO_DIE: SevenDaysToDieDriver = SevenDaysToDieDriver;

pub fn driver(game: GameId) -> &'static dyn GameDriver {
    match game {
        GameId::Valheim => &VALHEIM,
        GameId::Rust => &RUST,
        GameId::VRising => &V_RISING,
        GameId::Palworld => &PALWORLD,
        GameId::RunescapeDragonwilds => &RUNESCAPE_DRAGONWILDS,
        GameId::SevenDaysToDie => &SEVEN_DAYS_TO_DIE,
    }
}

pub fn drivers() -> [&'static dyn GameDriver; 6] {
    [
        &VALHEIM,
        &RUST,
        &V_RISING,
        &PALWORLD,
        &RUNESCAPE_DRAGONWILDS,
        &SEVEN_DAYS_TO_DIE,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

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
