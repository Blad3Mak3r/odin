//! Launch contracts shared by the first compiled, schema-driven game drivers.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::time::Duration;

use crate::db::game_instances::GenericGameInstance;
use crate::game::{GameId, driver};
use crate::paths::Paths;

pub fn prepare_start(
    _paths: &Paths,
    _db: &crate::db::Db,
    game: GameId,
    _name: &str,
) -> Result<GenericGameInstance> {
    bail!(
        "{} runs in Podman; start it through its container runtime",
        driver(game).display_name()
    )
}

pub async fn start(
    paths: &Paths,
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> Result<GenericGameInstance> {
    let instance = prepare_start(paths, db, game, name)?;
    crate::supervisor::client::spawn_detached(paths, &instance.identity).await?;
    crate::supervisor::client::ping_with_retry(paths, game, name, Duration::from_secs(10)).await?;
    crate::db::game_instances::load_generic(db, game, name)?
        .context("instance disappeared after start")
}

pub async fn stop(paths: &Paths, db: &crate::db::Db, game: GameId, name: &str) -> Result<()> {
    let instance = crate::db::game_instances::load_generic(db, game, name)?
        .context("game instance does not exist")?;
    let (Some(pid), Some(started_at)) = (instance.pid, instance.pid_started_at) else {
        bail!("instance '{name}' is not running");
    };
    // Palworld's REST shutdown tells the server to flush its world and exit
    // itself. Wait for that first, but retain the normal supervisor stop as a
    // bounded fallback if the REST API is unavailable or rejects the request.
    if game == GameId::Palworld
        && crate::game::palworld::rest_enabled(paths, &instance).unwrap_or(false)
    {
        let palworld = instance.clone();
        let palworld_paths = paths.clone();
        let rest_shutdown = tokio::task::spawn_blocking(move || {
            crate::game::palworld::shutdown(&palworld_paths, &palworld, Some(0), None)
        })
        .await;
        match rest_shutdown {
            Ok(result) => {
                if palworld_rest_shutdown_accepted(result, name)
                    && crate::instance::process::wait_until_gone(
                        pid,
                        started_at,
                        Duration::from_secs(40),
                    )
                    .await
                {
                    return Ok(());
                }
            }
            Err(error) => {
                tracing::warn!(
                    instance = name,
                    error = %error,
                    "Palworld REST shutdown task panicked; falling back to the supervisor"
                );
            }
        }
    }
    crate::supervisor::client::stop(paths, game, name, 30).await?;
    if !crate::instance::process::wait_until_gone(pid, started_at, Duration::from_secs(40)).await {
        bail!("instance '{name}' did not stop");
    }
    Ok(())
}

fn palworld_rest_shutdown_accepted(result: Result<Value>, name: &str) -> bool {
    match result {
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(
                instance = name,
                error = %error,
                "Palworld REST shutdown failed; falling back to the supervisor"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_palworld_rest_shutdown_is_not_terminal() {
        assert!(!palworld_rest_shutdown_accepted(
            Err(anyhow::anyhow!(
                "400 Bad Request: waittime is larger than 1"
            )),
            "palworld",
        ));
    }
}
