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
//! 4. Set `container_runtime: true` (and `stop_timeout_secs` if the game needs
//!    longer than 30 seconds to save) in the game's driver.
//!
//! No other host-side code enumerates container games. The full migration
//! checklist, with the traps each earlier game hit, is
//! `docs/container-game-migration.md`.

use anyhow::Result;

use crate::db::Db;
use crate::db::container_instances;
use crate::db::game_instances::GameInstanceIdentity;
use crate::game::GameId;
use crate::paths::Paths;
use crate::supervisor;

use super::lifecycle::{self, ContainerGame, RuntimeHealth, StopReason};
use super::{palworld, rust, seven_days_to_die, valheim};

/// Runs `$body` with `$game_type` bound to the [`ContainerGame`] of `$game`,
/// or evaluates `$otherwise` when that game does not run in a container.
macro_rules! for_container_game {
    ($game:expr, $game_type:ident => $body:expr, otherwise => $otherwise:expr) => {
        match $game {
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
            GameId::VRising | GameId::RunescapeDragonwilds => $otherwise,
        }
    };
}

fn unsupported(game: GameId) -> anyhow::Error {
    anyhow::anyhow!("{game} does not run in Podman")
}

/// Whether instances of `game` are managed by the container lifecycle.
pub fn is_container_game(game: GameId) -> bool {
    for_container_game!(game, G => { let _ = G::GAME; true }, otherwise => false)
}

pub fn migrate(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::migrate::<G>(paths, db, name),
        otherwise => Err(unsupported(game)))
}

pub async fn runtime_health(
    paths: &Paths,
    db: &Db,
    game: GameId,
    name: &str,
) -> Result<RuntimeHealth> {
    for_container_game!(game, G => lifecycle::runtime_health::<G>(paths, db, name).await,
        otherwise => Err(unsupported(game)))
}

pub async fn repair(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::repair::<G>(paths, db, name).await,
        otherwise => Err(unsupported(game)))
}

pub async fn update(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::update::<G>(paths, db, name).await,
        otherwise => Err(unsupported(game)))
}

/// Marks a newly created instance as containerized.
pub fn mark_new_instance(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::mark_new_instance::<G>(db, &instance)
    }, otherwise => Err(unsupported(game)))
}

pub fn is_migrated(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<bool> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::is_migrated::<G>(db, &instance)
    }, otherwise => Ok(false))
}

/// Authoritative liveness: the container for a migrated instance, the host
/// process for one that has not been migrated yet.
pub fn is_running(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<bool> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::is_running::<G>(paths, db, &instance)
    }, otherwise => Err(unsupported(game)))
}

pub async fn is_running_async(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<bool> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::is_running_async::<G>(paths, db, &instance).await
    }, otherwise => Err(unsupported(game)))
}

pub async fn start(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::start::<G>(paths, db, name).await.map(|_| ()),
        otherwise => Err(unsupported(game)))
}

/// Stops the container of a migrated instance. A server started before its
/// migration is still a host process and is stopped as one.
pub async fn stop(
    paths: &Paths,
    db: &Db,
    game: GameId,
    name: &str,
    reason: StopReason,
) -> Result<()> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        if lifecycle::is_migrated::<G>(db, &instance)? {
            lifecycle::stop::<G>(paths, db, name, reason).await
        } else {
            G::stop_directly(paths, db, &instance).await
        }
    }, otherwise => Err(unsupported(game)))
}

pub async fn restart(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => lifecycle::restart::<G>(paths, db, name).await.map(|_| ()),
        otherwise => Err(unsupported(game)))
}

/// Removes the container and the private runtime volume of a deleted instance.
pub fn remove(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::remove::<G>(paths, db, &instance)
    }, otherwise => Ok(()))
}

/// Rewrites the runtime spec of a migrated instance after its identity on
/// disk changed (a rename).
pub fn sync_spec(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    for_container_game!(game, G => {
        let instance = G::load(paths, db, name)?;
        lifecycle::sync_spec::<G>(paths, db, &instance)
    }, otherwise => Ok(()))
}

/// How an instance is executed right now, for views that must stay available
/// when Podman is unreachable.
#[derive(Default)]
pub struct Execution {
    pub migrated: bool,
    /// Whether the container is running. Always `false` before migration and
    /// when the container cannot be inspected.
    pub container_running: bool,
    pub odin_version: Option<String>,
}

pub fn execution(paths: &Paths, db: &Db, identity: &GameInstanceIdentity) -> Result<Execution> {
    if !container_instances::is_podman(db, &identity.id)? {
        return Ok(Execution::default());
    }
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
    Ok(Execution {
        migrated: true,
        container_running,
        odin_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_games_match_the_capability_flag() {
        for driver in crate::game::drivers() {
            assert_eq!(
                is_container_game(driver.id()),
                driver.capabilities().container_runtime,
                "{} capability and dispatch disagree",
                driver.id()
            );
        }
    }
}
