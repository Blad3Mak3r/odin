//! The one place that names every game running in a container.
//!
//! [`ContainerGame`] is generic over the game, so callers that only hold a
//! [`GameId`] go through the functions here instead of matching on the game
//! themselves.
//!
//! To run another game in a container:
//! 1. Implement [`ContainerGame`] in `podman/<game>.rs`.
//! 2. Add a `Launch` variant (tagged with the game's `GameId::as_str`) and its
//!    arms in `Launch::game`, `name`, `server_command` and
//!    `request_graceful_stop`, all in `podman.rs`.
//! 3. Add one arm to [`for_container_game!`] below.
//! 4. Override `stop_timeout_secs` in the game's driver if it needs longer
//!    than 30 seconds to save.
//!
//! No other host-side code enumerates container games. The full migration
//! checklist, with the traps each earlier game hit, is
//! `docs/container-game-migration.md`.

use anyhow::Result;

use crate::db::Db;
use crate::db::game_instances::GameInstanceIdentity;
use crate::game::GameId;
use crate::paths::Paths;
use crate::supervisor;

use super::lifecycle::{self, ContainerGame, RuntimeHealth, StopReason};
use super::{ark_asa, cs2, palworld, rust, seven_days_to_die, valheim};

/// Runs `$body` with `$game_type` bound to the [`ContainerGame`] of `$game`.
macro_rules! for_container_game {
    ($game:expr, $game_type:ident => $body:expr) => {
        match $game {
            GameId::ArkSurvivalAscended => {
                type $game_type = ark_asa::ArkSurvivalAscended;
                $body
            }
            GameId::Valheim => {
                type $game_type = valheim::Valheim;
                $body
            }
            GameId::Rust => {
                type $game_type = rust::Rust;
                $body
            }
            GameId::SevenDaysToDie => {
                type $game_type = seven_days_to_die::SevenDaysToDie;
                $body
            }
            GameId::Palworld => {
                type $game_type = palworld::Palworld;
                $body
            }
            GameId::Cs2 => {
                type $game_type = cs2::Cs2;
                $body
            }
        }
    };
}

pub async fn runtime_health(
    paths: &Paths,
    db: &Db,
    game: GameId,
    name: &str,
) -> Result<RuntimeHealth> {
    for_container_game!(game, G => lifecycle::runtime_health::<G>(paths, db, name).await)
}

pub async fn repair(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::repair::<G>(paths, db, name).await)
}

pub async fn update(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::update::<G>(paths, db, name).await)
}

/// Authoritative liveness: whether the instance's container is running.
pub fn is_running(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<bool> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::is_running::<G>(paths, db, &instance)
    })
}

pub async fn is_running_async(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<bool> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::is_running_async::<G>(paths, db, &instance).await
    })
}

pub async fn start(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::start::<G>(paths, db, name).await.map(|_| ()))
}

pub async fn stop(
    paths: &Paths,
    db: &Db,
    game: GameId,
    name: &str,
    reason: StopReason,
) -> Result<()> {
    for_container_game!(game, G => lifecycle::stop::<G>(paths, db, name, reason).await)
}

pub async fn restart(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::restart::<G>(paths, db, name).await.map(|_| ()))
}

/// Removes the container and the private runtime volume of a deleted instance.
pub fn remove(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::remove::<G>(paths, db, &instance)
    })
}

/// Rewrites the runtime spec of an instance after its identity on
/// disk changed (a rename).
pub fn sync_spec(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::sync_spec::<G>(paths, db, &instance)
    })
}

/// How an instance is executed right now, for views that must stay available
/// when Podman is unreachable.
#[derive(Default)]
pub struct Execution {
    /// Whether the container is running. `false` when it cannot be inspected.
    pub container_running: bool,
    pub odin_version: Option<String>,
    /// The running supervisor predates this Odin and only a restart (which
    /// re-mounts the host binary) brings it up to date. `false` when the
    /// supervisor cannot be reached: unknown is not outdated.
    pub restart_required: bool,
}

pub fn execution(paths: &Paths, identity: &GameInstanceIdentity) -> Execution {
    let container_running = lifecycle::container_is_running(paths, identity).unwrap_or_else(|error| {
        tracing::debug!(instance = %identity.name, %error, "failed to inspect Podman container");
        false
    });
    let pong = supervisor::client::ping_socket_blocking(
        &lifecycle::control_socket(paths, identity),
        std::time::Duration::from_millis(300),
    )
    .ok();
    let (odin_version, restart_required) = match pong {
        Some(supervisor::protocol::Response::Pong {
            protocol_version,
            odin_version,
            ..
        }) => {
            let outdated = container_running
                && supervisor_is_outdated(
                    protocol_version,
                    odin_version.as_deref(),
                    env!("CARGO_PKG_VERSION"),
                );
            (odin_version, outdated)
        }
        _ => (None, false),
    };
    Execution {
        container_running,
        odin_version,
        restart_required,
    }
}

fn supervisor_is_outdated(
    protocol_version: u16,
    supervisor_version: Option<&str>,
    current_version: &str,
) -> bool {
    protocol_version < supervisor::protocol::PROTOCOL_VERSION
        || supervisor_version
            .is_none_or(|version| crate::odin_update::is_newer(current_version, version))
}

#[cfg(test)]
mod supervisor_version_tests {
    use super::*;

    const PROTOCOL: u16 = supervisor::protocol::PROTOCOL_VERSION;

    #[test]
    fn missing_supervisor_version_is_outdated() {
        assert!(supervisor_is_outdated(PROTOCOL, None, "0.8.0"));
    }

    #[test]
    fn lower_supervisor_version_is_outdated() {
        assert!(supervisor_is_outdated(PROTOCOL, Some("0.7.0"), "0.8.0"));
    }

    #[test]
    fn older_protocol_is_outdated_even_with_the_current_version() {
        assert!(supervisor_is_outdated(PROTOCOL - 1, Some("0.8.0"), "0.8.0"));
    }

    #[test]
    fn current_or_newer_supervisor_is_not_outdated() {
        assert!(!supervisor_is_outdated(PROTOCOL, Some("0.8.0"), "0.8.0"));
        assert!(!supervisor_is_outdated(PROTOCOL, Some("0.9.0"), "0.8.0"));
    }
}
