//! Launch contracts shared by the first compiled, schema-driven game drivers.

use std::fs::OpenOptions;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use std::time::Duration;
use tokio::process::Command;

use crate::db::game_instances::GenericGameInstance;
use crate::game::{GameId, driver};
use crate::paths::{self, Paths};

pub fn prepare_start(
    paths: &Paths,
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> Result<GenericGameInstance> {
    let instance = crate::db::game_instances::load_generic(db, game, name)?
        .context("game instance does not exist")?;
    if instance.is_running() {
        bail!("instance '{name}' is already running");
    }
    let requested = [
        Some(instance.config.port),
        instance.config.query_port,
        instance.config.admin_port,
    ]
    .into_iter()
    .flatten();
    crate::game::ports::ensure_available(db, game, name, requested)?;
    let binary = paths
        .game_install_dir(game)
        .join(driver(game).server_binary());
    if !binary.is_file() {
        bail!(
            "{} is not installed (expected {})",
            driver(game).display_name(),
            binary.display()
        );
    }
    if game == GameId::RunescapeDragonwilds
        && instance
            .config
            .settings
            .get("owner_id")
            .and_then(|value| value.as_str())
            .is_none_or(str::is_empty)
    {
        bail!("RuneScape: Dragonwilds owner ID is required before starting an instance");
    }
    Ok(instance)
}

pub fn build_command(paths: &Paths, instance: &GenericGameInstance) -> Result<Command> {
    let game = instance.identity.game;
    let install_dir = paths.game_install_dir(game);
    let instance_dir = paths.game_instance_dir(game, instance.name());
    let log_dir = paths::instance_logs_dir(&instance_dir);
    std::fs::create_dir_all(&log_dir)?;
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("console.log"))?;
    let stderr = stdout.try_clone()?;
    let binary = install_dir.join(driver(game).server_binary());
    let mut command = match game {
        GameId::VRising => {
            let proton = paths.data_dir.join("runtimes/proton-ge/proton");
            if !proton.is_file() {
                bail!(
                    "Proton-GE is not installed at {}; install the managed runtime before starting V Rising",
                    proton.display()
                );
            }
            let mut command = Command::new(proton);
            command
                .arg("run")
                .arg(binary)
                .env("STEAM_COMPAT_DATA_PATH", instance_dir.join("proton-prefix"));
            command
        }
        GameId::Palworld | GameId::RunescapeDragonwilds => Command::new(binary),
        GameId::Valheim | GameId::Rust => unreachable!("generic command called for typed driver"),
    };
    command
        .current_dir(&install_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0);
    match game {
        GameId::VRising => {
            command
                .arg("-persistentDataPath")
                .arg(instance_dir.join("data"));
            command
                .arg("-gamePort")
                .arg(instance.config.port.to_string());
            if let Some(port) = instance.config.query_port {
                command.arg("-queryPort").arg(port.to_string());
            }
            if let Some(port) = instance.config.admin_port {
                command.arg("-rconPort").arg(port.to_string());
            }
        }
        GameId::Palworld => {
            command.arg(format!("-port={}", instance.config.port));
        }
        GameId::RunescapeDragonwilds => {
            command.arg("-log");
        }
        GameId::Valheim | GameId::Rust => unreachable!(),
    }
    Ok(command)
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
    crate::supervisor::client::stop(paths, game, name, 30).await?;
    if !crate::instance::process::wait_until_gone(pid, started_at, Duration::from_secs(40)).await {
        bail!("instance '{name}' did not stop");
    }
    Ok(())
}
