//! Launch contracts shared by the first compiled, schema-driven game drivers.

use std::fs::OpenOptions;
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
    if game == GameId::VRising {
        write_vrising_host_settings(paths, &instance)?;
    }
    Ok(instance)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::game_instances::{GameInstanceIdentity, GenericGameConfig};
    use chrono::Utc;

    #[test]
    fn vrising_host_settings_are_written_to_the_isolated_persistent_data_path() {
        let dir =
            std::env::temp_dir().join(format!("odin-vrising-settings-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let instance = GenericGameInstance {
            identity: GameInstanceIdentity {
                id: "id".into(),
                game: GameId::VRising,
                name: "vrising".into(),
                created_at: Utc::now(),
                tags: Vec::new(),
            },
            config: GenericGameConfig {
                port: 27015,
                query_port: Some(27016),
                admin_port: Some(25575),
                settings: json!({"server_name": "V Rising Test", "max_players": 40, "rcon_enabled": true}),
                auto_restart: false,
            },
            pid: None,
            pid_started_at: None,
            last_started_at: None,
            last_stopped_at: None,
        };

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
        assert_eq!(config["Rcon"]["BindAddress"], "127.0.0.1");
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
