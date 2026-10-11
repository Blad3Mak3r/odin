//! Launch contracts shared by the first compiled, schema-driven game drivers.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use serde_json::Value;
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
    // Validate the transport contract again at the lifecycle boundary.
    crate::db::game_instances::validate_generic_config(game, &instance.config)?;
    let requested = crate::db::game_instances::claimed_ports(game, &instance.config)?;
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
    match game {
        GameId::VRising => {
            crate::game::proton_ge::ensure(paths)?;
            crate::game::config_documents::initialize(paths, &instance)?;
        }
        GameId::Palworld | GameId::RunescapeDragonwilds => {
            prepare_native_runtime(paths, &instance)?;
            crate::game::config_documents::initialize(paths, &instance)?;
            if game == GameId::RunescapeDragonwilds {
                crate::game::config_documents::validate_dragonwilds(paths, &instance)?;
            }
        }
        GameId::SevenDaysToDie => {
            let instance_dir = paths.game_instance_dir(GameId::SevenDaysToDie, instance.name());
            let config = seven_d2d_config_path(paths, &instance);
            if let Some(parent) = config.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::create_dir_all(instance_dir.join("Saves"))?;
            fs::create_dir_all(instance_dir.join("Mods"))?;
            crate::game::config_documents::remove_legacy_control_panel(paths, &instance)?;
        }
        GameId::Valheim | GameId::Rust => unreachable!(),
    }
    crate::game::config_documents::sync_operational(paths, &instance)?;
    Ok(instance)
}

fn seven_d2d_config_path(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
    paths
        .game_instance_dir(GameId::SevenDaysToDie, instance.name())
        .join("config/serverconfig.xml")
}

fn native_runtime_dir(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
    paths
        .game_instance_dir(instance.identity.game, instance.name())
        .join("runtime")
}

/// Palworld and Dragonwilds write their Saved directory relative to their
/// install tree. Build a lightweight, per-instance runtime with hard links
/// to immutable Steam files and a private Saved directory, so several
/// servers never overwrite each other's configuration or worlds. The tree
/// is refreshed on every start, making SteamCMD updates visible immediately.
fn prepare_native_runtime(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    let source = paths.game_install_dir(instance.identity.game);
    let destination = native_runtime_dir(paths, instance);
    sync_runtime_tree(&source, &destination, instance.identity.game, Path::new(""))?;
    let saved = match instance.identity.game {
        GameId::Palworld => destination.join("Pal/Saved"),
        GameId::RunescapeDragonwilds => destination.join("RSDragonwilds/Saved"),
        GameId::Valheim | GameId::Rust | GameId::VRising | GameId::SevenDaysToDie => unreachable!(),
    };
    fs::create_dir_all(saved)?;
    Ok(())
}

fn sync_runtime_tree(
    source: &Path,
    destination: &Path,
    game: GameId,
    relative: &Path,
) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry?;
        let file_name = entry.file_name();
        let child_relative = relative.join(&file_name);
        if is_instance_data_path(game, &child_relative) {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(&file_name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            sync_runtime_tree(&source_path, &destination_path, game, &child_relative)?;
        } else if file_type.is_symlink() {
            replace_with_symlink(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            if destination_path.exists() || destination_path.symlink_metadata().is_ok() {
                fs::remove_file(&destination_path)?;
            }
            fs::hard_link(&source_path, &destination_path)
                .or_else(|_| fs::copy(&source_path, &destination_path).map(|_| ()))?;
        }
    }
    Ok(())
}

fn is_instance_data_path(game: GameId, relative: &Path) -> bool {
    match game {
        GameId::Palworld => relative.starts_with("Pal/Saved"),
        GameId::RunescapeDragonwilds => relative.starts_with("RSDragonwilds/Saved"),
        GameId::Valheim | GameId::Rust | GameId::VRising | GameId::SevenDaysToDie => false,
    }
}

#[cfg(unix)]
fn replace_with_symlink(source: &Path, destination: &Path) -> Result<()> {
    if destination.exists() || destination.symlink_metadata().is_ok() {
        if destination.is_dir() {
            fs::remove_dir_all(destination)?;
        } else {
            fs::remove_file(destination)?;
        }
    }
    std::os::unix::fs::symlink(fs::read_link(source)?, destination)?;
    Ok(())
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
    let runtime_dir = match game {
        GameId::Palworld | GameId::RunescapeDragonwilds => native_runtime_dir(paths, instance),
        GameId::Valheim | GameId::Rust | GameId::VRising | GameId::SevenDaysToDie => {
            install_dir.clone()
        }
    };
    let binary = runtime_dir.join(driver(game).server_binary());
    let mut command = match game {
        GameId::VRising => {
            let proton = crate::game::proton_ge::binary(paths);
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
        GameId::Palworld | GameId::RunescapeDragonwilds | GameId::SevenDaysToDie => {
            Command::new(binary)
        }
        GameId::Valheim | GameId::Rust => unreachable!("generic command called for typed driver"),
    };
    command
        .current_dir(&runtime_dir)
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
            let query_port = instance
                .config
                .query_port
                .context("Palworld Steam query port is required")?;
            command
                .arg(format!("-port={}", instance.config.port))
                .arg(format!("-queryport={query_port}"));
        }
        GameId::RunescapeDragonwilds => {
            command
                .arg("-log")
                .arg(format!("-Port={}", instance.config.port));
            if let Some(port) = instance.config.query_port {
                command.arg(format!("-BeaconPort={port}"));
            }
        }
        GameId::SevenDaysToDie => {
            command
                .arg(format!(
                    "-configfile={}",
                    seven_d2d_config_path(paths, instance).display()
                ))
                .arg(format!("-UserDataFolder={}", instance_dir.display()))
                .arg(format!("-ServerPort={}", instance.config.port))
                .arg(format!(
                    "-logfile={}",
                    log_dir.join("console.log").display()
                ))
                .arg("-quit")
                .arg("-batchmode")
                .arg("-nographics");
            if let Some(port) = instance.config.admin_port {
                command.arg(format!("-TelnetPort={port}"));
            }
            command.arg("-dedicated");
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
    use crate::db::game_instances::{GameInstanceIdentity, GenericGameConfig};
    use chrono::Utc;

    fn generic_instance(game: GameId) -> GenericGameInstance {
        GenericGameInstance {
            identity: GameInstanceIdentity {
                id: "id".into(),
                game,
                name: "server".into(),
                created_at: Utc::now(),
                tags: Vec::new(),
            },
            config: GenericGameConfig {
                port: if game == GameId::Palworld { 8211 } else { 7777 },
                query_port: match game {
                    GameId::Palworld => Some(27015),
                    GameId::RunescapeDragonwilds => Some(8888),
                    _ => None,
                },
                admin_port: None,
                auto_restart: false,
            },
            pid: None,
            pid_started_at: None,
            last_started_at: None,
            last_stopped_at: None,
        }
    }

    #[test]
    fn failed_palworld_rest_shutdown_is_not_terminal() {
        assert!(!palworld_rest_shutdown_accepted(
            Err(anyhow::anyhow!(
                "400 Bad Request: waittime is larger than 1"
            )),
            "palworld",
        ));
    }

    #[test]
    fn palworld_runtime_keeps_saved_data_isolated_while_refreshing_steam_files() {
        let dir =
            std::env::temp_dir().join(format!("odin-palworld-runtime-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let install = paths.game_install_dir(GameId::Palworld);
        fs::create_dir_all(install.join("Pal/Saved")).unwrap();
        fs::write(install.join("PalServer.sh"), "server-v1").unwrap();
        fs::write(install.join("Pal/Saved/shared.txt"), "must-not-copy").unwrap();
        let instance = generic_instance(GameId::Palworld);

        prepare_native_runtime(&paths, &instance).unwrap();
        let runtime = native_runtime_dir(&paths, &instance);
        assert_eq!(
            fs::read_to_string(runtime.join("PalServer.sh")).unwrap(),
            "server-v1"
        );
        assert!(!runtime.join("Pal/Saved/shared.txt").exists());
        assert!(
            !runtime
                .join("Pal/Saved/Config/LinuxServer/PalWorldSettings.ini")
                .exists()
        );
        fs::create_dir_all(runtime.join("Pal/Saved/SaveGames")).unwrap();
        fs::write(runtime.join("Pal/Saved/SaveGames/world.sav"), "world").unwrap();
        fs::write(install.join("PalServer.sh"), "server-v2").unwrap();
        prepare_native_runtime(&paths, &instance).unwrap();
        assert_eq!(
            fs::read_to_string(runtime.join("PalServer.sh")).unwrap(),
            "server-v2"
        );
        assert_eq!(
            fs::read_to_string(runtime.join("Pal/Saved/SaveGames/world.sav")).unwrap(),
            "world"
        );
        fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn dragonwilds_command_passes_both_udp_ports() {
        let dir =
            std::env::temp_dir().join(format!("odin-dragon-command-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let instance = generic_instance(GameId::RunescapeDragonwilds);

        let command = build_command(&paths, &instance).unwrap();
        let arguments = format!("{command:?}");
        assert!(arguments.contains("-Port=7777"));
        assert!(arguments.contains("-BeaconPort=8888"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn vrising_command_uses_persistent_data_and_all_operational_ports() {
        let dir =
            std::env::temp_dir().join(format!("odin-vrising-command-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let proton = crate::game::proton_ge::binary(&paths);
        fs::create_dir_all(proton.parent().unwrap()).unwrap();
        fs::write(&proton, "").unwrap();
        let mut instance = generic_instance(GameId::VRising);
        instance.config.port = 27015;
        instance.config.query_port = Some(27016);
        instance.config.admin_port = Some(25575);

        let args: Vec<_> = build_command(&paths, &instance)
            .unwrap()
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        for expected in [
            ["-gamePort", "27015"],
            ["-queryPort", "27016"],
            ["-rconPort", "25575"],
        ] {
            assert!(args.windows(2).any(|pair| pair == expected));
        }
        assert!(
            args.windows(2).any(|pair| pair[0] == "-persistentDataPath"
                && pair[1].ends_with("instances/server/data"))
        );
        fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn palworld_command_passes_a_private_steam_query_port() {
        let dir =
            std::env::temp_dir().join(format!("odin-palworld-command-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let instance = generic_instance(GameId::Palworld);

        let command = build_command(&paths, &instance).unwrap();
        let arguments = format!("{command:?}");
        assert!(arguments.contains("-port=8211"));
        assert!(arguments.contains("-queryport=27015"));
        fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn seven_days_command_uses_instance_specific_paths_without_creating_config() {
        let dir = std::env::temp_dir().join(format!("odin-7d2d-settings-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let mut instance = generic_instance(GameId::SevenDaysToDie);
        instance.identity.name = "undead".into();
        instance.config.port = 26900;

        let root = paths.game_instance_dir(GameId::SevenDaysToDie, "undead");
        let command = build_command(&paths, &instance).unwrap();
        let arguments = format!("{command:?}");
        assert!(arguments.contains(&format!(
            "-configfile={}",
            root.join("config/serverconfig.xml").display()
        )));
        assert!(arguments.contains(&format!("-UserDataFolder={}", root.display())));
        assert!(!root.join("config/serverconfig.xml").exists());
        assert_eq!(
            command
                .as_std()
                .get_args()
                .last()
                .unwrap()
                .to_string_lossy(),
            "-dedicated"
        );
        fs::remove_dir_all(paths.data_dir).ok();
    }
}
