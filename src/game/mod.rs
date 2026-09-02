//! Statically compiled game definitions.  A driver describes only facts that
//! are common to the host; game-specific state stays in its own module.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub mod rust;
pub mod update;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameId {
    Valheim,
    Rust,
}

impl GameId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valheim => "valheim",
            Self::Rust => "rust",
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

pub trait GameDriver: Sync {
    fn id(&self) -> GameId;
    fn display_name(&self) -> &'static str;
    fn steam_app_id(&self) -> &'static str;
    fn server_binary(&self) -> &'static str;
    fn capabilities(&self) -> GameCapabilities;
}

struct ValheimDriver;
struct RustDriver;

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
            access_lists: false,
            readiness: false,
        }
    }
}

static VALHEIM: ValheimDriver = ValheimDriver;
static RUST: RustDriver = RustDriver;

pub fn driver(game: GameId) -> &'static dyn GameDriver {
    match game {
        GameId::Valheim => &VALHEIM,
        GameId::Rust => &RUST,
    }
}

pub fn drivers() -> [&'static dyn GameDriver; 2] {
    [&VALHEIM, &RUST]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_driver_has_the_dedicated_server_app_id() {
        assert_eq!(driver(GameId::Rust).steam_app_id(), "258550");
    }
}
