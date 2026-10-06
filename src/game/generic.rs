//! Launch contracts shared by the first compiled, schema-driven game drivers.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
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
    // Validate persisted settings again at the lifecycle boundary. This
    // protects instances created with defaults (before their first form
    // submission) and any data imported from an older Odin version.
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
            let rcon_enabled = instance
                .config
                .settings
                .get("rcon_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if rcon_enabled
                && instance
                    .config
                    .settings
                    .get("rcon_password")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            {
                bail!("V Rising RCON password is required when RCON is enabled");
            }
            crate::game::proton_ge::ensure(paths)?;
            write_vrising_host_settings(paths, &instance)?;
        }
        GameId::Palworld | GameId::RunescapeDragonwilds => {
            prepare_native_runtime(paths, &instance)?;
            write_native_settings(paths, &instance)?;
        }
        GameId::SevenDaysToDie => write_7d2d_settings(paths, &instance)?,
        GameId::Valheim | GameId::Rust => unreachable!(),
    }
    Ok(instance)
}

fn seven_d2d_config_path(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
    paths
        .game_instance_dir(GameId::SevenDaysToDie, instance.name())
        .join("config/serverconfig.xml")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn set_xml_property(contents: &mut String, name: &str, value: &str) -> Result<()> {
    let expression = format!(
        r#"(?s)<property\s+name\s*=\s*"{}"[^>]*>"#,
        regex::escape(name)
    );
    let replacement = format!(r#"<property name="{name}" value="{}"/>"#, xml_escape(value));
    let pattern = regex::Regex::new(&expression)?;
    if pattern.is_match(contents) {
        *contents = pattern
            .replace(contents, |_: &regex::Captures<'_>| replacement.clone())
            .into_owned();
    } else if let Some(index) = contents.rfind("</ServerSettings>") {
        contents.insert_str(index, &format!("    {replacement}\n"));
    } else {
        bail!("serverconfig.xml has no ServerSettings element");
    }
    Ok(())
}

fn write_7d2d_settings(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    let instance_dir = paths.game_instance_dir(GameId::SevenDaysToDie, instance.name());
    let config = seven_d2d_config_path(paths, instance);
    if let Some(parent) = config.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut contents = if config.is_file() {
        fs::read_to_string(&config)?
    } else {
        let template = paths
            .game_install_dir(GameId::SevenDaysToDie)
            .join("serverconfig.xml");
        if template.is_file() {
            fs::read_to_string(template)?
        } else {
            "<?xml version=\"1.0\"?>\n<ServerSettings>\n</ServerSettings>\n".into()
        }
    };
    let settings = &instance.config.settings;
    let string = |key: &str, fallback: &str| setting_string(settings, key, fallback);
    let number =
        |key: &str, fallback: u64| setting_u64(settings, key).unwrap_or(fallback).to_string();
    for (name, value) in [
        ("ServerName", string("server_name", instance.name())),
        ("ServerDescription", string("server_description", "")),
        ("ServerPassword", string("server_password", "")),
        ("ServerVisibility", number("visibility", 2)),
        ("ServerMaxPlayerCount", number("max_players", 8)),
        ("ServerPort", instance.config.port.to_string()),
        ("GameWorld", string("game_world", "Navezgane")),
        ("GameName", string("game_name", instance.name())),
        ("WorldGenSeed", string("world_gen_seed", instance.name())),
        ("WorldGenSize", number("world_gen_size", 6144)),
        ("UserDataFolder", instance_dir.display().to_string()),
        (
            "SaveGameFolder",
            instance_dir.join("Saves").display().to_string(),
        ),
        ("TelnetEnabled", "false".into()),
        ("ControlPanelEnabled", "false".into()),
        ("WebDashboardEnabled", "false".into()),
    ] {
        set_xml_property(&mut contents, name, &value)?;
    }
    fs::create_dir_all(instance_dir.join("Saves"))?;
    fs::create_dir_all(instance_dir.join("Mods"))?;
    fs::write(&config, contents).with_context(|| format!("failed to write {}", config.display()))
}

/// V Rising deliberately supports a per-instance persistent-data directory.
/// Keep Odin's generated host override there rather than modifying Steam's
/// install tree, which would couple every managed V Rising instance.
fn write_vrising_host_settings(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    let settings_dir = paths
        .game_instance_dir(GameId::VRising, instance.name())
        .join("data/Settings");
    std::fs::create_dir_all(&settings_dir)?;

    let settings = &instance.config.settings;
    let mut host = Map::new();
    host.insert(
        "Name".into(),
        Value::String(setting_string(settings, "server_name", instance.name())),
    );
    host.insert("Port".into(), json!(instance.config.port));
    if let Some(port) = instance.config.query_port {
        host.insert("QueryPort".into(), json!(port));
    }
    if let Some(max_players) = setting_u64(settings, "max_players") {
        host.insert("MaxConnectedUsers".into(), json!(max_players));
    }
    if let Some(port) = instance.config.admin_port {
        host.insert(
            "Rcon".into(),
            json!({
                "Enabled": settings.get("rcon_enabled").and_then(Value::as_bool).unwrap_or(false),
                "Port": port,
                "Password": setting_string(settings, "rcon_password", ""),
                "BindAddress": "127.0.0.1",
            }),
        );
    }
    let file = settings_dir.join("ServerHostSettings.json");
    std::fs::write(&file, serde_json::to_vec_pretty(&Value::Object(host))?)
        .with_context(|| format!("failed to write {}", file.display()))?;
    Ok(())
}

fn setting_string(settings: &Value, key: &str, fallback: &str) -> String {
    settings
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn setting_u64(settings: &Value, key: &str) -> Option<u64> {
    settings.get(key).and_then(Value::as_u64)
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

fn write_native_settings(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    match instance.identity.game {
        GameId::Palworld => write_palworld_settings(paths, instance),
        GameId::RunescapeDragonwilds => write_dragonwilds_settings(paths, instance),
        GameId::Valheim | GameId::Rust | GameId::VRising | GameId::SevenDaysToDie => unreachable!(),
    }
}

fn write_palworld_settings(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    let settings_file = native_runtime_dir(paths, instance)
        .join("Pal/Saved/Config/LinuxServer/PalWorldSettings.ini");
    let parent = settings_file
        .parent()
        .expect("Palworld settings has a parent");
    fs::create_dir_all(parent)?;
    let server_name = ini_string(&setting_string(
        &instance.config.settings,
        "server_name",
        instance.name(),
    ));
    let max_players = setting_u64(&instance.config.settings, "max_players").unwrap_or(32);
    let rest_enabled = instance
        .config
        .settings
        .get("rest_api_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let rest_port = instance.config.admin_port.unwrap_or(8212);
    let admin_password = ini_string(&setting_string(
        &instance.config.settings,
        "admin_password",
        "",
    ));
    let server_password = ini_string(&setting_string(
        &instance.config.settings,
        "server_password",
        "",
    ));
    let contents = format!(
        "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(ServerName=\"{server_name}\",ServerPlayerMaxNum={max_players},PublicPort={},AdminPassword=\"{admin_password}\",ServerPassword=\"{server_password}\",RESTAPIEnabled={},RESTAPIPort={rest_port})\n",
        instance.config.port,
        if rest_enabled { "True" } else { "False" },
    );
    fs::write(&settings_file, contents)
        .with_context(|| format!("failed to write {}", settings_file.display()))
}

fn write_dragonwilds_settings(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    let settings_file = native_runtime_dir(paths, instance)
        .join("RSDragonwilds/Saved/Config/Linux/DedicatedServer.ini");
    let parent = settings_file
        .parent()
        .expect("Dragonwilds settings has a parent");
    fs::create_dir_all(parent)?;
    let settings = &instance.config.settings;
    let contents = format!(
        "[/Script/Dominion.DedicatedServerSettings]\nOwnerId={}\nServerName={}\nDefaultWorldName={}\nAdminPassword={}\nDefaultWorldPassword={}\n",
        ini_string(&setting_string(settings, "owner_id", "")),
        ini_string(&setting_string(settings, "server_name", instance.name())),
        ini_string(&setting_string(
            settings,
            "default_world_name",
            instance.name()
        )),
        ini_string(&setting_string(settings, "admin_password", "")),
        ini_string(&setting_string(settings, "world_password", "")),
    );
    fs::write(&settings_file, contents)
        .with_context(|| format!("failed to write {}", settings_file.display()))
}

fn ini_string(value: &str) -> String {
    value.replace(['\r', '\n'], "").replace('"', "\\\"")
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
            command.arg(format!("-port={}", instance.config.port));
        }
        GameId::RunescapeDragonwilds => {
            command
                .arg("-log")
                .arg(format!("-port={}", instance.config.port));
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
                .arg(format!(
                    "-logfile={}",
                    log_dir.join("console.log").display()
                ))
                .arg("-quit")
                .arg("-batchmode")
                .arg("-nographics")
                .arg("-dedicated");
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
    if game == GameId::Palworld && crate::game::palworld::rest_enabled(&instance) {
        let palworld = instance.clone();
        let rest_shutdown = tokio::task::spawn_blocking(move || {
            crate::game::palworld::shutdown(&palworld, Some(0), None)
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

    fn generic_instance(game: GameId, settings: Value) -> GenericGameInstance {
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
                query_port: if game == GameId::RunescapeDragonwilds {
                    Some(8888)
                } else {
                    None
                },
                admin_port: if game == GameId::Palworld {
                    Some(8212)
                } else {
                    None
                },
                settings,
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
    fn vrising_host_settings_are_written_to_the_isolated_persistent_data_path() {
        let dir =
            std::env::temp_dir().join(format!("odin-vrising-settings-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let mut instance = generic_instance(
            GameId::VRising,
            json!({"server_name": "V Rising Test", "max_players": 40, "rcon_enabled": true, "rcon_password": "password"}),
        );
        instance.identity.name = "vrising".into();
        instance.config.port = 27015;
        instance.config.query_port = Some(27016);
        instance.config.admin_port = Some(25575);

        write_vrising_host_settings(&paths, &instance).unwrap();

        let config: Value = serde_json::from_slice(
            &std::fs::read(
                paths
                    .game_instance_dir(GameId::VRising, "vrising")
                    .join("data/Settings/ServerHostSettings.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(config["Name"], "V Rising Test");
        assert_eq!(config["Port"], 27015);
        assert_eq!(config["QueryPort"], 27016);
        assert_eq!(config["MaxConnectedUsers"], 40);
        assert_eq!(config["Rcon"]["Port"], 25575);
        assert_eq!(config["Rcon"]["Password"], "password");
        assert_eq!(config["Rcon"]["BindAddress"], "127.0.0.1");
        std::fs::remove_dir_all(paths.data_dir).ok();
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
        let instance = generic_instance(
            GameId::Palworld,
            json!({"server_name": "Pals", "max_players": 24, "rest_api_enabled": true, "server_password": "join-secret"}),
        );

        prepare_native_runtime(&paths, &instance).unwrap();
        write_native_settings(&paths, &instance).unwrap();
        let runtime = native_runtime_dir(&paths, &instance);
        assert_eq!(
            fs::read_to_string(runtime.join("PalServer.sh")).unwrap(),
            "server-v1"
        );
        assert!(!runtime.join("Pal/Saved/shared.txt").exists());
        assert!(
            fs::read_to_string(runtime.join("Pal/Saved/Config/LinuxServer/PalWorldSettings.ini"))
                .unwrap()
                .contains("ServerName=\"Pals\"")
        );
        assert!(
            fs::read_to_string(runtime.join("Pal/Saved/Config/LinuxServer/PalWorldSettings.ini"))
                .unwrap()
                .contains("ServerPassword=\"join-secret\"")
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
    fn dragonwilds_settings_use_the_linux_file_and_required_keys() {
        let dir =
            std::env::temp_dir().join(format!("odin-dragon-settings-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let instance = generic_instance(
            GameId::RunescapeDragonwilds,
            json!({"owner_id": "owner", "server_name": "Dragon", "default_world_name": "World", "admin_password": "admin", "world_password": "join"}),
        );

        write_dragonwilds_settings(&paths, &instance).unwrap();

        let settings = fs::read_to_string(
            native_runtime_dir(&paths, &instance)
                .join("RSDragonwilds/Saved/Config/Linux/DedicatedServer.ini"),
        )
        .unwrap();
        assert!(settings.contains("OwnerId=owner"));
        assert!(settings.contains("DefaultWorldPassword=join"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn dragonwilds_command_passes_both_udp_ports() {
        let dir =
            std::env::temp_dir().join(format!("odin-dragon-command-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let instance = generic_instance(
            GameId::RunescapeDragonwilds,
            json!({"owner_id": "owner", "server_name": "Dragon", "default_world_name": "world", "admin_password": "secret"}),
        );

        let command = build_command(&paths, &instance).unwrap();
        let arguments = format!("{command:?}");
        assert!(arguments.contains("-port=7777"));
        assert!(arguments.contains("-BeaconPort=8888"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn seven_days_settings_preserve_the_template_and_isolate_data() {
        let dir = std::env::temp_dir().join(format!("odin-7d2d-settings-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let install = paths.game_install_dir(GameId::SevenDaysToDie);
        fs::create_dir_all(&install).unwrap();
        fs::write(
            install.join("serverconfig.xml"),
            "<ServerSettings>\n  <property name=\"UnmanagedSetting\" value=\"keep\"/>\n</ServerSettings>\n",
        )
        .unwrap();
        let mut instance = generic_instance(
            GameId::SevenDaysToDie,
            json!({
                "server_name": "Undead Test",
                "server_description": "Private test server",
                "server_password": "secret",
                "visibility": 0,
                "max_players": 12,
                "game_world": "RWG",
                "game_name": "OdinWorld",
                "world_gen_seed": "seed",
                "world_gen_size": 8192,
            }),
        );
        instance.identity.name = "undead".into();
        instance.config.port = 26900;

        write_7d2d_settings(&paths, &instance).unwrap();

        let root = paths.game_instance_dir(GameId::SevenDaysToDie, "undead");
        let settings = fs::read_to_string(root.join("config/serverconfig.xml")).unwrap();
        assert!(settings.contains("UnmanagedSetting\" value=\"keep"));
        assert!(settings.contains("ServerName\" value=\"Undead Test"));
        assert!(settings.contains("ServerPort\" value=\"26900"));
        assert!(settings.contains(&format!("UserDataFolder\" value=\"{}", root.display())));
        assert!(settings.contains("TelnetEnabled\" value=\"false"));
        assert!(root.join("Saves").is_dir());
        assert!(root.join("Mods").is_dir());
        fs::remove_dir_all(paths.data_dir).ok();
    }
}
