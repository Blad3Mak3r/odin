//! Palworld's side of the dedicated-server contract: its launch command, the
//! native files it reads, and local, authenticated access to its REST API.
//!
//! Odin deliberately never proxies the REST service to an arbitrary host:
//! every request is made to the managed instance's loopback REST port.
//! Enablement, port, and password are read directly from PalWorldSettings.ini.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Method;
use reqwest::blocking::RequestBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db::game_instances::{GameInstanceIdentity, GenericGameInstance};
use crate::game::GameId;
use crate::paths::Paths;

const REST_TIMEOUT: Duration = Duration::from_secs(10);
const TEMPLATE_FILE: &str = "DefaultPalWorldSettings.ini";

/// Relative to the instance directory. Palworld writes `Pal/Saved` beside its
/// binaries, so a containerized server reaches this directory through a link
/// (see [`prepare_runtime`]).
pub const SAVED_DIR: &str = "runtime/Pal/Saved";
pub const CONFIG_FILE: &str = "runtime/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini";

/// The launch arguments Odin passes to the server itself. Every other setting
/// lives in the game's own `PalWorldSettings.ini`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PalworldLaunch {
    pub name: String,
    pub port: u16,
    pub query_port: u16,
}

impl TryFrom<&GenericGameInstance> for PalworldLaunch {
    type Error = anyhow::Error;

    fn try_from(instance: &GenericGameInstance) -> Result<Self> {
        Ok(Self {
            name: instance.name().to_string(),
            port: instance.config.port,
            query_port: instance
                .config
                .query_port
                .context("Palworld Steam query port is required")?,
        })
    }
}

/// The single definition of the dedicated server's launch contract. The
/// working directory is the install tree, where `PalServer.sh` expects to run.
pub fn build_command_at(
    install_dir: &Path,
    console_log: &Path,
    launch: &PalworldLaunch,
) -> Result<tokio::process::Command> {
    let binary = install_dir.join(crate::game::driver(GameId::Palworld).server_binary());
    if !binary.is_file() {
        bail!("Palworld is not installed (expected {})", binary.display());
    }
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(console_log)
        .with_context(|| format!("failed to open {}", console_log.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to duplicate Palworld log handle")?;
    let mut command = tokio::process::Command::new(binary);
    command
        .current_dir(install_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr))
        .process_group(0)
        .arg(format!("-port={}", launch.port))
        .arg(format!("-queryport={}", launch.query_port));
    Ok(command)
}

/// Where the installed server's `DefaultPalWorldSettings.ini` template lives
/// for an instance: its private copy of the game once installed, otherwise a
/// template placed in the host install directory.
pub fn template_path(paths: &Paths, identity: &GameInstanceIdentity) -> PathBuf {
    let private = paths
        .container_runtime_dir(GameId::Palworld, &identity.id)
        .join(GameId::Palworld.as_str())
        .join(TEMPLATE_FILE);
    if private.is_file() {
        private
    } else {
        paths.game_install_dir(GameId::Palworld).join(TEMPLATE_FILE)
    }
}

/// Container-side preparation, run before every launch: the game is installed
/// in the private runtime volume by now. Its `Pal/Saved` becomes a link to the
/// instance directory, which holds the saves and the native configuration, and
/// a missing `PalWorldSettings.ini` is created from the shipped template. An
/// existing file is never touched.
pub fn prepare_runtime(install_dir: &Path, instance_dir: &Path) -> Result<()> {
    let saved = instance_dir.join(SAVED_DIR);
    fs::create_dir_all(&saved).with_context(|| format!("failed to create {}", saved.display()))?;
    let bridge = install_dir.join("Pal/Saved");
    fs::create_dir_all(bridge.parent().context("Palworld bridge has no parent")?)?;
    match bridge.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(&bridge)? != saved {
                fs::remove_file(&bridge)?;
                std::os::unix::fs::symlink(&saved, &bridge)
                    .with_context(|| format!("failed to create {}", bridge.display()))?;
            }
        }
        Ok(_) => bail!(
            "{} exists and is not a symlink; refusing to replace it",
            bridge.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(&saved, &bridge)
                .with_context(|| format!("failed to create {}", bridge.display()))?;
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect {}", bridge.display()));
        }
    }
    crate::game::config_documents::copy_template_if_missing(
        &install_dir.join(TEMPLATE_FILE),
        &instance_dir.join(CONFIG_FILE),
    )
}

/// The loopback REST endpoint of one server, built from its own configuration.
struct RestApi {
    port: u16,
    password: String,
}

impl RestApi {
    fn from_config(read: impl Fn(&str) -> Result<Option<String>>) -> Result<Self> {
        let setting =
            |key: &str| read(key)?.context("Palworld configuration has not been generated yet");
        if !is_enabled(&setting("RESTAPIEnabled")?) {
            bail!("Palworld REST API is disabled for this instance");
        }
        let password = setting("AdminPassword")?;
        anyhow::ensure!(
            !password.is_empty(),
            "Palworld REST API requires an administrator password"
        );
        let port = setting("RESTAPIPort")?
            .parse::<u16>()
            .context("Palworld REST API port is invalid")?;
        Ok(Self { port, password })
    }

    fn for_instance(paths: &Paths, instance: &GenericGameInstance) -> Result<Self> {
        Self::from_config(|key| crate::game::config_documents::value(paths, instance, key))
    }

    fn request(&self, method: Method, endpoint: &str) -> RequestBuilder {
        let url = format!("http://127.0.0.1:{}/v1/api/{endpoint}", self.port);
        crate::http::CLIENT
            .request(method, url)
            .basic_auth("admin", Some(&self.password))
            .timeout(REST_TIMEOUT)
    }
}

fn is_enabled(value: &str) -> bool {
    matches!(value, "True" | "true" | "1")
}

fn request(
    paths: &Paths,
    instance: &GenericGameInstance,
    endpoint: &str,
) -> Result<RequestBuilder> {
    Ok(RestApi::for_instance(paths, instance)?.request(Method::POST, endpoint))
}

fn get(paths: &Paths, instance: &GenericGameInstance, endpoint: &str) -> Result<RequestBuilder> {
    Ok(RestApi::for_instance(paths, instance)?.request(Method::GET, endpoint))
}

fn send(request: RequestBuilder, operation: &str) -> Result<Value> {
    let response = request
        .send()
        .with_context(|| format!("failed to contact Palworld REST API for {operation}"))?;
    let status = response.status();
    let body = response
        .text()
        .with_context(|| format!("failed to read Palworld REST API response for {operation}"))?;
    if !status.is_success() {
        bail!("Palworld REST API {operation} failed with {status}: {body}");
    }
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&body)
        .with_context(|| format!("Palworld REST API returned invalid JSON for {operation}"))
}

pub fn players(paths: &crate::paths::Paths, instance: &GenericGameInstance) -> Result<Value> {
    send(get(paths, instance, "players")?, "list players")
}

pub fn metrics(paths: &crate::paths::Paths, instance: &GenericGameInstance) -> Result<Value> {
    send(get(paths, instance, "metrics")?, "read metrics")
}

pub fn announce(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    message: &str,
) -> Result<Value> {
    send(
        request(paths, instance, "announce")?.json(&json!({"message": message})),
        "announce",
    )
}

pub fn save(paths: &crate::paths::Paths, instance: &GenericGameInstance) -> Result<Value> {
    send(
        request(paths, instance, "save")?.json(&json!({})),
        "save world",
    )
}

pub fn kick(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    user_id: &str,
    message: Option<&str>,
) -> Result<Value> {
    player_action(paths, instance, "kick", user_id, message)
}

pub fn ban(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    user_id: &str,
    message: Option<&str>,
) -> Result<Value> {
    player_action(paths, instance, "ban", user_id, message)
}

pub fn unban(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    user_id: &str,
) -> Result<Value> {
    send(
        request(paths, instance, "unban")?.json(&json!({"userid": user_id})),
        "unban player",
    )
}

fn player_action(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    action: &str,
    user_id: &str,
    message: Option<&str>,
) -> Result<Value> {
    let mut payload = serde_json::Map::from_iter([(String::from("userid"), json!(user_id))]);
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        payload.insert(String::from("message"), json!(message));
    }
    send(request(paths, instance, action)?.json(&payload), action)
}

pub fn shutdown(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    wait_time: Option<u32>,
    message: Option<&str>,
) -> Result<Value> {
    let mut payload = serde_json::Map::new();
    if let Some(wait_time) = wait_time {
        payload.insert(String::from("waittime"), json!(wait_time));
    }
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        payload.insert(String::from("message"), json!(message));
    }
    send(
        request(paths, instance, "shutdown")?.json(&payload),
        "shutdown server",
    )
}

/// Asks a running server to save its world and exit, using the REST settings
/// in the instance's own `PalWorldSettings.ini`. This is the container
/// runtime's graceful stop, which only knows the instance directory.
pub fn shutdown_at(instance_dir: &Path) -> Result<()> {
    let config = instance_dir.join(CONFIG_FILE);
    let api =
        RestApi::from_config(|key| crate::game::config_documents::palworld_value(&config, key))?;
    send(
        api.request(Method::POST, "shutdown")
            .json(&json!({"waittime": 0})),
        "shutdown server",
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::game_instances::{GameInstanceIdentity, GenericGameConfig};
    use crate::game::GameId;
    use chrono::Utc;

    #[test]
    fn disabled_rest_api_is_never_contacted() {
        let instance = GenericGameInstance {
            identity: GameInstanceIdentity {
                id: "id".into(),
                game: GameId::Palworld,
                name: "pal".into(),
                created_at: Utc::now(),
                tags: vec![],
            },
            config: GenericGameConfig {
                port: 8211,
                query_port: None,
                admin_port: None,
                auto_restart: false,
            },
            pid: None,
            pid_started_at: None,
            last_started_at: None,
            last_stopped_at: None,
        };
        let dir = std::env::temp_dir().join(format!("odin-palworld-rest-{}", uuid::Uuid::new_v4()));
        let paths = crate::paths::Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let config = paths
            .game_instance_dir(GameId::Palworld, "pal")
            .join("runtime/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(config, "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(RESTAPIEnabled=False,AdminPassword=secret,RESTAPIPort=8212)\n").unwrap();
        assert!(
            players(&paths, &instance)
                .unwrap_err()
                .to_string()
                .contains("disabled")
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("odin-palworld-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const TEMPLATE: &str = "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(ServerName=\"Default\",RESTAPIEnabled=False)\n";

    #[test]
    fn launch_command_is_the_complete_server_contract() {
        let install = temp_dir("command");
        fs::write(install.join("PalServer.sh"), "").unwrap();
        let log = install.join("console.log");
        let launch = PalworldLaunch {
            name: "pals".into(),
            port: 8211,
            query_port: 27015,
        };

        let command = build_command_at(&install, &log, &launch).unwrap();
        let command = command.as_std();

        assert_eq!(command.get_program(), install.join("PalServer.sh"));
        assert_eq!(command.get_current_dir(), Some(install.as_path()));
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args, ["-port=8211", "-queryport=27015"]);
        fs::remove_dir_all(install).ok();
    }

    #[test]
    fn launch_command_requires_an_installed_server() {
        let install = temp_dir("missing");
        let launch = PalworldLaunch {
            name: "pals".into(),
            port: 8211,
            query_port: 27015,
        };
        let error = build_command_at(&install, &install.join("console.log"), &launch).unwrap_err();
        assert!(error.to_string().contains("not installed"));
        fs::remove_dir_all(install).ok();
    }

    #[test]
    fn runtime_links_saved_into_the_instance_and_seeds_a_missing_config() {
        let install = temp_dir("install");
        let instance = temp_dir("instance");
        fs::write(install.join(TEMPLATE_FILE), TEMPLATE).unwrap();

        prepare_runtime(&install, &instance).unwrap();
        prepare_runtime(&install, &instance).unwrap();

        let saved = instance.join(SAVED_DIR);
        assert_eq!(fs::read_link(install.join("Pal/Saved")).unwrap(), saved);
        // Writes through the game's own path land in the instance directory.
        fs::write(install.join("Pal/Saved/world.sav"), "world").unwrap();
        assert_eq!(
            fs::read_to_string(saved.join("world.sav")).unwrap(),
            "world"
        );
        assert_eq!(
            fs::read_to_string(instance.join(CONFIG_FILE)).unwrap(),
            TEMPLATE
        );
        fs::remove_dir_all(install).ok();
        fs::remove_dir_all(instance).ok();
    }

    #[test]
    fn runtime_never_replaces_an_operator_config_or_existing_saves() {
        let install = temp_dir("install-keep");
        let instance = temp_dir("instance-keep");
        fs::write(install.join(TEMPLATE_FILE), TEMPLATE).unwrap();
        let config = instance.join(CONFIG_FILE);
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "operator").unwrap();
        fs::create_dir_all(instance.join(SAVED_DIR).join("SaveGames")).unwrap();
        fs::write(
            instance.join(SAVED_DIR).join("SaveGames/world.sav"),
            "world",
        )
        .unwrap();

        prepare_runtime(&install, &instance).unwrap();

        assert_eq!(fs::read_to_string(&config).unwrap(), "operator");
        assert_eq!(
            fs::read_to_string(install.join("Pal/Saved/SaveGames/world.sav")).unwrap(),
            "world"
        );
        fs::remove_dir_all(install).ok();
        fs::remove_dir_all(instance).ok();
    }

    #[test]
    fn runtime_without_a_template_fails_only_when_a_config_is_needed() {
        let install = temp_dir("install-bare");
        let instance = temp_dir("instance-bare");
        let error = prepare_runtime(&install, &instance).unwrap_err();
        assert!(error.to_string().contains("template not found"));

        let config = instance.join(CONFIG_FILE);
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "operator").unwrap();
        prepare_runtime(&install, &instance).unwrap();
        fs::remove_dir_all(install).ok();
        fs::remove_dir_all(instance).ok();
    }

    #[test]
    fn runtime_refuses_to_replace_a_real_saved_directory() {
        let install = temp_dir("install-real");
        let instance = temp_dir("instance-real");
        fs::write(install.join(TEMPLATE_FILE), TEMPLATE).unwrap();
        fs::create_dir_all(install.join("Pal/Saved")).unwrap();

        let error = prepare_runtime(&install, &instance).unwrap_err();

        assert!(error.to_string().contains("not a symlink"));
        fs::remove_dir_all(install).ok();
        fs::remove_dir_all(instance).ok();
    }

    #[test]
    fn template_prefers_the_private_runtime_copy() {
        let dir = temp_dir("template");
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let identity = GameInstanceIdentity {
            id: "id".into(),
            game: GameId::Palworld,
            name: "pals".into(),
            created_at: Utc::now(),
            tags: vec![],
        };
        let shared = paths.game_install_dir(GameId::Palworld).join(TEMPLATE_FILE);
        assert_eq!(template_path(&paths, &identity), shared);

        let private = paths
            .container_runtime_dir(GameId::Palworld, "id")
            .join("palworld")
            .join(TEMPLATE_FILE);
        fs::create_dir_all(private.parent().unwrap()).unwrap();
        fs::write(&private, TEMPLATE).unwrap();
        assert_eq!(template_path(&paths, &identity), private);
        fs::remove_dir_all(dir).ok();
    }
}
