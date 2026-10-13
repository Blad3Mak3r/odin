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
use super::{ark_asa, palworld, rust, seven_days_to_die, valheim};

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
}

pub fn execution(paths: &Paths, identity: &GameInstanceIdentity) -> Execution {
    let container_running = lifecycle::container_is_running(paths, identity).unwrap_or_else(|error| {
        tracing::debug!(instance = %identity.name, %error, "failed to inspect Podman container");
        false
    });
    let odin_version = supervisor::client::ping_socket_blocking(
        &lifecycle::control_socket(paths, identity),
        std::time::Duration::from_millis(300),
    )
    .ok()
    .and_then(|response| match response {
        supervisor::protocol::Response::Pong { odin_version, .. } => odin_version,
        _ => None,
    });
    Execution {
        container_running,
        odin_version,
    }
}
