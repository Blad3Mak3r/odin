//! Podman lifecycle for Valheim. Existing non-migrated instances are kept
//! stopped until an operator explicitly adopts them through `migrate`.

use std::os::unix::fs::FileTypeExt;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::config::{GlobalConfig, PodmanConfig};
use crate::db::Db;
use crate::db::container_instances;
use crate::db::game_instances::{self, GameInstanceIdentity};
use crate::db::resource_limits;
use crate::game::GameId;
use crate::instance::{Instance, InstanceError, lifecycle};
use crate::paths::{self, Paths};
use crate::supervisor;

use super::{ContainerSpec, ContainerState, PodmanClient, container_name, write_runtime_spec};

const START_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const STOP_TIMEOUT_SECS: u64 = 30;

#[derive(Serialize)]
pub struct RuntimeHealth {
    pub podman: RuntimePodmanHealth,
    pub container: RuntimeContainerHealth,
    pub socket: RuntimeSocketHealth,
    pub guidance: Vec<String>,
}

#[derive(Serialize)]
pub struct RuntimePodmanHealth {
    pub configured: bool,
    pub connected: bool,
    pub compatible: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct RuntimeContainerHealth {
    pub status: &'static str,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct RuntimeSocketHealth {
    pub status: &'static str,
    pub protocol_version: Option<u16>,
    pub odin_version: Option<String>,
    pub error: Option<String>,
}

fn config(paths: &Paths) -> Result<PodmanConfig> {
    GlobalConfig::load(paths)?
        .podman
        .context("Valheim requires [podman].socket; configure access to Podman's Unix socket")
}

fn identity(db: &Db, instance: &Instance) -> Result<GameInstanceIdentity> {
    game_instances::ensure_valheim_identity(db, &instance.state.name, instance.state.created_at)
}

fn require_migrated(db: &Db, identity: &GameInstanceIdentity) -> Result<()> {
    if container_instances::is_valheim_podman(db, &identity.id)? {
        Ok(())
    } else {
        bail!(
            "Valheim instance '{}' has not been migrated to Podman; stop it and use the migration action first",
            identity.name
        )
    }
}

fn prepare_layout(
    paths: &Paths,
    identity: &GameInstanceIdentity,
    instance: &Instance,
) -> Result<()> {
    std::fs::create_dir_all(paths::instance_saves_dir(&instance.dir))?;
    std::fs::create_dir_all(paths::instance_logs_dir(&instance.dir))?;
    std::fs::create_dir_all(paths.valheim_runtime_dir(&identity.id))?;
    std::fs::create_dir_all(paths.valheim_socket_dir(&identity.id))?;
    std::fs::create_dir_all(paths.mods_dir())?;
    write_runtime_spec(paths, identity, instance)?;
    Ok(())
}

/// Adopts a stopped legacy Valheim instance without reading, copying, or
/// removing its shared host installation. The marker is inserted only after
/// all resumable host directories and the runtime spec are prepared.
pub fn migrate(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let _lock = lifecycle::LifecycleLock::acquire(paths, GameId::Valheim, name)?;
    let instance = Instance::load_existing(paths, db, name)?;
    if lifecycle::is_running(&instance)? {
        bail!(InstanceError::AlreadyRunning(name.to_string()));
    }
    let identity = identity(db, &instance)?;
    if container_instances::is_valheim_podman(db, &identity.id)? {
        return Ok(());
    }
    prepare_layout(paths, &identity, &instance)?;
    container_instances::mark_valheim_podman(db, &identity.id)?;
    Ok(())
}

/// Marks a newly-created Valheim instance as containerized. It does not pull
/// an image or create a container, so creation remains offline and cheap.
pub fn mark_new_instance(db: &Db, instance: &Instance) -> Result<()> {
    let identity = identity(db, instance)?;
    container_instances::mark_valheim_podman(db, &identity.id)
}

pub fn is_migrated(db: &Db, instance: &Instance) -> Result<bool> {
    let identity = identity(db, instance)?;
    container_instances::is_valheim_podman(db, &identity.id)
}

/// Reports every boundary needed to start a migrated Valheim runtime without
/// mutating Podman, the runtime volume, or the instance data directory.
pub async fn runtime_health(paths: &Paths, db: &Db, name: &str) -> Result<RuntimeHealth> {
    let instance = Instance::load_existing(paths, db, name)?;
    let identity = identity(db, &instance)?;
    require_migrated(db, &identity)?;
    let control_socket = control_socket(paths, &identity);

    let config = GlobalConfig::load(paths)?.podman;
    let Some(config) = config else {
        return Ok(RuntimeHealth {
            podman: RuntimePodmanHealth {
                configured: false,
                connected: false,
                compatible: false,
                version: None,
                error: None,
            },
            container: unavailable_container(),
            socket: socket_health(&control_socket, None).await,
            guidance: vec![
                "Configure Podman's Unix socket in Settings before starting Valheim.".into(),
            ],
        });
    };
    let client = match PodmanClient::from_config(&config) {
        Ok(client) => client,
        Err(error) => {
            return Ok(RuntimeHealth {
                podman: RuntimePodmanHealth {
                    configured: true,
                    connected: false,
                    compatible: false,
                    version: None,
                    error: Some(error.to_string()),
                },
                container: unavailable_container(),
                socket: socket_health(&control_socket, None).await,
                guidance: vec!["Fix the configured Podman socket before starting Valheim.".into()],
            });
        }
    };
    let version = match client.version().await {
        Ok(version) => version,
        Err(error) => {
            return Ok(RuntimeHealth {
                podman: RuntimePodmanHealth {
                    configured: true,
                    connected: false,
                    compatible: false,
                    version: None,
                    error: Some(error.to_string()),
                },
                container: unavailable_container(),
                socket: socket_health(&control_socket, None).await,
                guidance: vec![
                    "Restore access to Podman's Unix socket before starting Valheim.".into(),
                ],
            });
        }
    };
    let compatible = version.is_supported();
    let podman = RuntimePodmanHealth {
        configured: true,
        connected: true,
        compatible,
        version: Some(format!("{} (API {})", version.version, version.api_version)),
        error: (!compatible).then(|| "Podman 4.0.0 or newer is required".into()),
    };
    if !compatible {
        return Ok(RuntimeHealth {
            podman,
            container: unavailable_container(),
            socket: socket_health(&control_socket, None).await,
            guidance: vec![
                "Upgrade Podman to version 4.0.0 or newer before starting Valheim.".into(),
            ],
        });
    }
    let state = match client.inspect(&container_name(&identity.id)).await {
        Ok(state) => state,
        Err(error) => {
            return Ok(RuntimeHealth {
                podman,
                container: RuntimeContainerHealth {
                    status: "unavailable",
                    error: Some(error.to_string()),
                },
                socket: socket_health(&control_socket, None).await,
                guidance: vec!["Check the Podman service and try the diagnostic again.".into()],
            });
        }
    };
    let container = RuntimeContainerHealth {
        status: state.map_or("missing", ContainerState::as_str),
        error: None,
    };
    let socket = socket_health(&control_socket, state).await;
    let guidance = runtime_guidance(container.status, socket.status);
    Ok(RuntimeHealth {
        podman,
        container,
        socket,
        guidance,
    })
}

pub async fn repair(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let _lock = lifecycle::LifecycleLock::acquire(paths, GameId::Valheim, name)?;
    let instance = Instance::load_existing(paths, db, name)?;
    let identity = identity(db, &instance)?;
    require_migrated(db, &identity)?;
    if lifecycle::is_running(&instance)? {
        bail!(InstanceError::AlreadyRunning(name.to_string()));
    }
    let config = config(paths)?;
    let client = PodmanClient::from_config(&config)?;
    client.ensure_supported().await?;
    let container = container_name(&identity.id);
    match client.inspect(&container).await? {
        Some(ContainerState::Running) => bail!(InstanceError::AlreadyRunning(name.to_string())),
        Some(ContainerState::Unknown) => {
            bail!(
                "refusing to repair Valheim instance '{name}': Podman returned an unknown container state"
            )
        }
        Some(ContainerState::Created | ContainerState::Exited) => {
            prepare_layout(paths, &identity, &instance)?;
            clean_socket_dir(&paths.valheim_socket_dir(&identity.id))?;
            client.remove(&container, false).await?;
        }
        None => {
            prepare_layout(paths, &identity, &instance)?;
            clean_socket_dir(&paths.valheim_socket_dir(&identity.id))?;
        }
    }
    Ok(())
}

pub fn sync_spec(paths: &Paths, db: &Db, instance: &Instance) -> Result<()> {
    let identity = identity(db, instance)?;
    if container_instances::is_valheim_podman(db, &identity.id)? {
        prepare_layout(paths, &identity, instance)?;
    }
    Ok(())
}

pub fn is_running(paths: &Paths, db: &Db, instance: &Instance) -> Result<bool> {
    let identity = identity(db, instance)?;
    if !container_instances::is_valheim_podman(db, &identity.id)? {
        return lifecycle::is_running(instance);
    }
    let Some(config) = GlobalConfig::load(paths)?.podman else {
        return Ok(false);
    };
    let client = PodmanClient::from_config(&config)?;
    Ok(client
        .inspect_blocking(&container_name(&identity.id))?
        .is_some_and(ContainerState::is_running))
}

async fn is_running_async(paths: &Paths, db: &Db, instance: &Instance) -> Result<bool> {
    let identity = identity(db, instance)?;
    if !container_instances::is_valheim_podman(db, &identity.id)? {
        return lifecycle::is_running(instance);
    }
    let client = PodmanClient::from_config(&config(paths)?)?;
    Ok(client
        .inspect(&container_name(&identity.id))
        .await?
        .is_some_and(ContainerState::is_running))
}

pub async fn start(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    let _lock = lifecycle::LifecycleLock::acquire(paths, GameId::Valheim, name)?;
    let instance = Instance::load_existing(paths, db, name)?;
    let identity = identity(db, &instance)?;
    require_migrated(db, &identity)?;
    crate::game::ports::ensure_available(
        db,
        GameId::Valheim,
        name,
        crate::game::ports::block(GameId::Valheim, instance.state.port)?,
    )?;
    prepare_layout(paths, &identity, &instance)?;

    let config = config(paths)?;
    let client = PodmanClient::from_config(&config)?;
    client.ensure_supported().await?;
    let limits = resource_limits::load(db, &identity.id)?;
    let container = container_name(&identity.id);
    let image = config.runtime_image();
    let spec = ContainerSpec::valheim(paths, &identity, &instance, image.clone(), &limits);
    match client.inspect(&container).await? {
        Some(state) if state.is_running() => bail!(InstanceError::AlreadyRunning(name.to_string())),
        Some(_) => {
            client.update_limits(&container, &limits).await?;
            client.start(&container).await?;
        }
        None => {
            client.ensure_image(&image).await?;
            client.create(&spec).await?;
            client.start(&container).await?;
        }
    }
    let response = supervisor::client::ping_socket_with_retry(
        &control_socket(paths, &identity),
        START_TIMEOUT,
    )
    .await
    .with_context(|| {
        format!("container {container} started but its Odin runtime did not respond")
    })?;
    match response {
        crate::supervisor::protocol::Response::Pong {
            protocol_version, ..
        } if protocol_version >= crate::supervisor::protocol::PROTOCOL_VERSION => {}
        crate::supervisor::protocol::Response::Pong {
            protocol_version, ..
        } => bail!(
            "runtime image protocol {protocol_version} is incompatible; require protocol {} or newer",
            crate::supervisor::protocol::PROTOCOL_VERSION
        ),
        other => bail!("runtime image returned an unexpected startup response: {other:?}"),
    }
    Ok(instance)
}

pub async fn stop(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let _lock = lifecycle::LifecycleLock::acquire(paths, GameId::Valheim, name)?;
    let instance = Instance::load_existing(paths, db, name)?;
    let identity = identity(db, &instance)?;
    require_migrated(db, &identity)?;
    let config = config(paths)?;
    let client = PodmanClient::from_config(&config)?;
    let container = container_name(&identity.id);
    let state = client.inspect(&container).await?;
    let Some(state) = state else {
        bail!(InstanceError::NotRunning(name.to_string()));
    };
    if !state.is_running() {
        bail!(InstanceError::NotRunning(name.to_string()));
    }
    let socket = control_socket(paths, &identity);
    if supervisor::client::stop_socket(&socket, STOP_TIMEOUT_SECS)
        .await
        .is_err()
    {
        client.stop(&container, STOP_TIMEOUT_SECS).await?;
    }
    Ok(())
}

pub async fn restart(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    let instance = Instance::load_existing(paths, db, name)?;
    if is_running_async(paths, db, &instance).await? {
        stop(paths, db, name).await?;
    }
    start(paths, db, name).await
}

/// Updates the game payload inside an already-running runtime. The container
/// remains alive; the runtime stops and restarts only its Valheim child.
pub async fn update(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let instance = Instance::load_existing(paths, db, name)?;
    let identity = identity(db, &instance)?;
    require_migrated(db, &identity)?;
    if !is_running_async(paths, db, &instance).await? {
        bail!("Valheim update requires a running instance; Odin will not start it implicitly");
    }
    supervisor::client::update_socket(&control_socket(paths, &identity)).await
}

pub async fn update_limits(paths: &Paths, db: &Db, instance_id: &str) -> Result<()> {
    if !container_instances::is_valheim_podman(db, instance_id)? {
        return Ok(());
    }
    let config = config(paths)?;
    let client = PodmanClient::from_config(&config)?;
    if client
        .inspect(&container_name(instance_id))
        .await?
        .is_none()
    {
        return Ok(());
    }
    let limits = resource_limits::load(db, instance_id)?;
    client
        .update_limits(&container_name(instance_id), &limits)
        .await
}

pub fn remove(paths: &Paths, db: &Db, instance: &Instance) -> Result<()> {
    let identity = identity(db, instance)?;
    if !container_instances::is_valheim_podman(db, &identity.id)? {
        return Ok(());
    }
    let config = config(paths)?;
    PodmanClient::from_config(&config)?.remove_blocking(&container_name(&identity.id), true)
}

fn unavailable_container() -> RuntimeContainerHealth {
    RuntimeContainerHealth {
        status: "unavailable",
        error: None,
    }
}

async fn socket_health(
    control_socket: &std::path::Path,
    container: Option<ContainerState>,
) -> RuntimeSocketHealth {
    let is_socket = std::fs::symlink_metadata(control_socket)
        .map(|metadata| metadata.file_type().is_socket())
        .unwrap_or(false);
    if !is_socket {
        let has_stale_socket = control_socket.parent().is_some_and(socket_dir_has_socket);
        return RuntimeSocketHealth {
            status: if !container.is_some_and(ContainerState::is_running) && has_stale_socket {
                "orphaned"
            } else {
                "missing"
            },
            protocol_version: None,
            odin_version: None,
            error: None,
        };
    }
    if !container.is_some_and(ContainerState::is_running) {
        return RuntimeSocketHealth {
            status: "orphaned",
            protocol_version: None,
            odin_version: None,
            error: None,
        };
    }
    match supervisor::client::ping_socket(control_socket).await {
        Ok(crate::supervisor::protocol::Response::Pong {
            protocol_version,
            odin_version,
            ..
        }) => RuntimeSocketHealth {
            status: "responding",
            protocol_version: Some(protocol_version),
            odin_version,
            error: None,
        },
        Ok(response) => RuntimeSocketHealth {
            status: "unreachable",
            protocol_version: None,
            odin_version: None,
            error: Some(format!("unexpected runtime response: {response:?}")),
        },
        Err(error) => RuntimeSocketHealth {
            status: "unreachable",
            protocol_version: None,
            odin_version: None,
            error: Some(error.to_string()),
        },
    }
}

fn runtime_guidance(container: &str, socket: &str) -> Vec<String> {
    match (container, socket) {
        (_, "orphaned") => {
            vec!["Use Repair to remove the stale runtime socket before the next start.".into()]
        }
        ("missing", _) => vec!["Start the instance to create its Podman container.".into()],
        ("running", "responding") => vec!["The Podman runtime is healthy.".into()],
        ("running", _) => vec![
            "The container is running but its runtime socket is unavailable; restart the instance."
                .into(),
        ],
        ("created" | "exited", _) => vec![
            "The stopped container can be removed safely with Repair before the next start.".into(),
        ],
        ("unknown", _) => {
            vec!["Inspect the container in Podman before attempting a repair.".into()]
        }
        _ => vec!["Re-run the diagnostic after configuring Podman.".into()],
    }
}

fn socket_dir_has_socket(socket_dir: &std::path::Path) -> bool {
    std::fs::read_dir(socket_dir).ok().is_some_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_socket())
                .unwrap_or(false)
        })
    })
}

fn clean_socket_dir(socket_dir: &std::path::Path) -> Result<()> {
    if !socket_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(socket_dir)
        .with_context(|| format!("failed to read {}", socket_dir.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_socket() {
            std::fs::remove_file(entry.path())
                .with_context(|| format!("failed to remove {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn control_socket(paths: &Paths, identity: &GameInstanceIdentity) -> std::path::PathBuf {
    paths.valheim_socket_dir(&identity.id).join(format!(
        "{}-{}.control.sock",
        GameId::Valheim,
        identity.name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    fn write_podman_config(paths: &Paths, socket: std::path::PathBuf) {
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        let config = crate::config::GlobalConfig {
            data_dir: None,
            podman: Some(crate::config::PodmanConfig {
                socket,
                runtime_image: None,
            }),
        };
        std::fs::write(
            paths.config_file(),
            toml::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn clean_socket_dir_removes_only_unix_sockets() {
        let directory =
            std::env::temp_dir().join(format!("odin-runtime-health-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("stale.sock");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake runtime socket: {error}"),
        };
        let keep = directory.join("keep.txt");
        std::fs::write(&keep, "keep").unwrap();

        clean_socket_dir(&directory).unwrap();

        assert!(!socket.exists());
        assert_eq!(std::fs::read_to_string(&keep).unwrap(), "keep");
        drop(listener);
        std::fs::remove_dir_all(directory).ok();
    }

    #[tokio::test]
    async fn stopped_container_socket_is_reported_as_orphaned() {
        let directory =
            std::env::temp_dir().join(format!("odin-runtime-health-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("stale.sock");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake runtime socket: {error}"),
        };

        let health = socket_health(&socket, Some(ContainerState::Exited)).await;

        assert_eq!(health.status, "orphaned");
        drop(listener);
        std::fs::remove_dir_all(directory).ok();
    }

    #[tokio::test]
    async fn repair_removes_only_the_stopped_container_and_ephemeral_sockets() {
        let root =
            std::env::temp_dir().join(format!("odin-runtime-repair-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config_dir: root.clone(),
            data_dir: root.clone(),
        };
        let podman_socket = root.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                std::fs::remove_dir_all(root).ok();
                return;
            }
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let (status, body) = if request.contains("/version") {
                    ("200 OK", r#"{"Version":"4.0.0","ApiVersion":"4.0"}"#)
                } else if request.contains("/json") {
                    ("200 OK", r#"{"State":{"Running":false,"Status":"exited"}}"#)
                } else {
                    ("204 No Content", "")
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                requests.push(request);
            }
            requests
        });
        write_podman_config(&paths, podman_socket.clone());

        let db = crate::db::Db::open(&paths).unwrap();
        let instance = Instance::create(&paths, &db, "valheim").unwrap();
        let identity = identity(&db, &instance).unwrap();
        container_instances::mark_valheim_podman(&db, &identity.id).unwrap();
        let saves = paths::instance_saves_dir(&instance.dir);
        std::fs::create_dir_all(&saves).unwrap();
        let save = saves.join("world.db");
        std::fs::write(&save, "save data").unwrap();
        let backups = paths::instance_backups_dir(&instance.dir);
        std::fs::create_dir_all(&backups).unwrap();
        let backup = backups.join("world.zip");
        std::fs::write(&backup, "backup data").unwrap();
        let mods = paths.mods_dir();
        std::fs::create_dir_all(&mods).unwrap();
        let mod_file = mods.join("example-mod");
        std::fs::write(&mod_file, "mod data").unwrap();
        let socket_dir = paths.valheim_socket_dir(&identity.id);
        std::fs::create_dir_all(&socket_dir).unwrap();
        let stale_socket = socket_dir.join("stale.sock");
        let stale_listener = UnixListener::bind(&stale_socket).unwrap();

        repair(&paths, &db, "valheim").await.unwrap();

        assert_eq!(std::fs::read_to_string(save).unwrap(), "save data");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "backup data");
        assert_eq!(std::fs::read_to_string(mod_file).unwrap(), "mod data");
        assert!(crate::podman::runtime_spec_path(&paths, &identity.id).is_file());
        assert!(!stale_socket.exists());
        let requests = server.join().unwrap();
        assert!(
            requests
                .iter()
                .any(|request| request.starts_with("DELETE "))
        );
        drop(stale_listener);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn migration_gate_rejects_an_unmigrated_instance() {
        let root = std::env::temp_dir().join(format!(
            "odin-runtime-repair-unmigrated-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config_dir: root.clone(),
            data_dir: root.clone(),
        };
        let db = crate::db::Db::open(&paths).unwrap();
        let instance = Instance::create(&paths, &db, "valheim").unwrap();
        let identity = identity(&db, &instance).unwrap();

        let error = require_migrated(&db, &identity).unwrap_err();

        assert!(
            error.to_string().contains("has not been migrated"),
            "unexpected migration error: {error:#}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn repair_rejects_a_running_container() {
        let root = std::env::temp_dir().join(format!(
            "odin-runtime-repair-running-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config_dir: root.clone(),
            data_dir: root.clone(),
        };
        let podman_socket = root.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                std::fs::remove_dir_all(root).ok();
                return;
            }
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let body = if request.contains("/version") {
                    r#"{"Version":"4.0.0","ApiVersion":"4.0"}"#
                } else {
                    r#"{"State":{"Running":true,"Status":"running"}}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                requests.push(request);
            }
            requests
        });
        write_podman_config(&paths, podman_socket);
        let db = crate::db::Db::open(&paths).unwrap();
        let instance = Instance::create(&paths, &db, "valheim").unwrap();
        let identity = identity(&db, &instance).unwrap();
        container_instances::mark_valheim_podman(&db, &identity.id).unwrap();

        let error = repair(&paths, &db, "valheim").await.unwrap_err();

        assert!(error.to_string().contains("already running"));
        let requests = server.join().unwrap();
        assert!(
            requests
                .iter()
                .all(|request| !request.starts_with("DELETE "))
        );
        std::fs::remove_dir_all(root).ok();
    }
}
