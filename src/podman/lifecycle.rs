//! Game-neutral Podman lifecycle. A game only describes the few facts that
//! differ ([`ContainerGame`]); creating, starting, stopping, repairing and
//! probing its container is the same for every game. Instances that were not
//! explicitly migrated are kept stopped until an operator adopts them through
//! `migrate`.

use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tokio::time::{Instant, sleep};

use crate::config::{GlobalConfig, PodmanConfig};
use crate::db::Db;
use crate::db::container_instances;
use crate::db::game_instances::GameInstanceIdentity;
use crate::db::resource_limits;
use crate::game::{self, GameId};
use crate::instance::InstanceError;
use crate::instance::lifecycle::LifecycleLock;
use crate::paths::Paths;
use crate::supervisor;

use super::{ContainerSpec, ContainerState, Mount, PodmanClient, container_name};

const START_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const CONTAINER_CREATE_TIMEOUT: Duration = Duration::from_secs(5);
const CONTAINER_CREATE_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Slack after the runtime's own deadline, so a server that needed all of its
/// time is not mistaken for a hung container.
const STOP_GRACE: Duration = Duration::from_secs(5);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// What differs between games that run in a container. Everything else in
/// this module is shared.
pub trait ContainerGame {
    const GAME: GameId;
    type Instance;

    fn load(paths: &Paths, db: &Db, name: &str) -> Result<Self::Instance>;
    fn identity(db: &Db, instance: &Self::Instance) -> Result<GameInstanceIdentity>;
    /// Host directory bind-mounted as `/srv/odin/instance`.
    fn instance_dir(paths: &Paths, instance: &Self::Instance) -> PathBuf;
    /// Every port the server binds, for the conflict check before starting.
    fn ports(instance: &Self::Instance) -> Result<Vec<u16>>;
    /// Creates the host directories and writes the runtime spec. Safe to
    /// repeat: it runs before every start, repair and migration.
    fn prepare_layout(
        paths: &Paths,
        identity: &GameInstanceIdentity,
        instance: &Self::Instance,
    ) -> Result<()>;
    /// Mounts only this game needs, on top of the shared ones.
    fn extra_mounts(paths: &Paths) -> Vec<Mount>;
    /// Whether the pre-migration host process is alive.
    fn running_directly(instance: &Self::Instance) -> Result<bool>;
}

/// Why a container is being stopped, which decides whether Odin remembers that
/// the instance should still be running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// An operator asked for it to stay down.
    Operator,
    /// The stop is one step of a restart.
    Restart,
    /// Odin or the host is shutting down; the instance should come back.
    HostShutdown,
}

impl StopReason {
    fn clears_desired_running(self) -> bool {
        self == Self::Operator
    }
}

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

fn display(game: GameId) -> &'static str {
    game::driver(game).display_name()
}

fn config(paths: &Paths, game: GameId) -> Result<PodmanConfig> {
    GlobalConfig::load(paths)?.podman.with_context(|| {
        format!(
            "{} requires [podman].socket; configure access to Podman's Unix socket",
            display(game)
        )
    })
}

pub(super) fn require_migrated(db: &Db, identity: &GameInstanceIdentity) -> Result<()> {
    if container_instances::is_podman(db, &identity.id)? {
        Ok(())
    } else {
        bail!(
            "{} instance '{}' has not been migrated to Podman; stop it and use the migration action first",
            display(identity.game),
            identity.name
        )
    }
}

/// Adopts a stopped legacy instance without reading, copying, or removing its
/// shared host installation. The marker is inserted only after all resumable
/// host directories and the runtime spec are prepared.
pub fn migrate<G: ContainerGame>(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, G::GAME, name)?;
    let instance = G::load(paths, db, name)?;
    if G::running_directly(&instance)? {
        bail!(InstanceError::AlreadyRunning(name.to_string()));
    }
    let identity = G::identity(db, &instance)?;
    if container_instances::is_podman(db, &identity.id)? {
        return Ok(());
    }
    G::prepare_layout(paths, &identity, &instance)?;
    container_instances::mark_podman(db, &identity.id)?;
    Ok(())
}

/// Marks a newly-created instance as containerized. It does not pull an image
/// or create a container, so creation remains offline and cheap.
pub fn mark_new_instance<G: ContainerGame>(db: &Db, instance: &G::Instance) -> Result<()> {
    let identity = G::identity(db, instance)?;
    container_instances::mark_podman(db, &identity.id)
}

pub fn is_migrated<G: ContainerGame>(db: &Db, instance: &G::Instance) -> Result<bool> {
    let identity = G::identity(db, instance)?;
    container_instances::is_podman(db, &identity.id)
}

/// Reports every boundary needed to start a migrated runtime without mutating
/// Podman, the runtime volume, or the instance data directory.
pub async fn runtime_health<G: ContainerGame>(
    paths: &Paths,
    db: &Db,
    name: &str,
) -> Result<RuntimeHealth> {
    let instance = G::load(paths, db, name)?;
    let identity = G::identity(db, &instance)?;
    require_migrated(db, &identity)?;
    let control_socket = control_socket(paths, &identity);
    let game = display(G::GAME);

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
            guidance: vec![format!(
                "Configure Podman's Unix socket in Settings before starting {game}."
            )],
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
                guidance: vec![format!(
                    "Fix the configured Podman socket before starting {game}."
                )],
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
                guidance: vec![format!(
                    "Restore access to Podman's Unix socket before starting {game}."
                )],
            });
        }
    };
    let compatible = version.is_supported();
    let podman = RuntimePodmanHealth {
        configured: true,
        connected: true,
        compatible,
        version: Some(format!("{} (API {})", version.version, version.api_version)),
        error: (!compatible).then(|| "Podman 5.0.0 or newer is required".into()),
    };
    if !compatible {
        return Ok(RuntimeHealth {
            podman,
            container: unavailable_container(),
            socket: socket_health(&control_socket, None).await,
            guidance: vec![format!(
                "Upgrade Podman to version 5.0.0 or newer before starting {game}."
            )],
        });
    }
    let state = match client.inspect(&container_name(G::GAME, &identity.id)).await {
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

pub async fn repair<G: ContainerGame>(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, G::GAME, name)?;
    let instance = G::load(paths, db, name)?;
    let identity = G::identity(db, &instance)?;
    require_migrated(db, &identity)?;
    let config = config(paths, G::GAME)?;
    let client = PodmanClient::from_config(&config)?;
    client.ensure_supported().await?;
    let container = container_name(G::GAME, &identity.id);
    match client.inspect(&container).await? {
        Some(ContainerState::Running) => bail!(InstanceError::AlreadyRunning(name.to_string())),
        Some(ContainerState::Unknown) => {
            bail!(
                "refusing to repair {} instance '{name}': Podman returned an unknown container state",
                display(G::GAME)
            )
        }
        Some(ContainerState::Created | ContainerState::Exited) => {
            G::prepare_layout(paths, &identity, &instance)?;
            clean_socket_dir(&paths.container_socket_dir(&identity.id))?;
            client.remove(&container, false).await?;
        }
        None => {
            G::prepare_layout(paths, &identity, &instance)?;
            clean_socket_dir(&paths.container_socket_dir(&identity.id))?;
        }
    }
    Ok(())
}

pub fn sync_spec<G: ContainerGame>(paths: &Paths, db: &Db, instance: &G::Instance) -> Result<()> {
    let identity = G::identity(db, instance)?;
    if container_instances::is_podman(db, &identity.id)? {
        G::prepare_layout(paths, &identity, instance)?;
    }
    Ok(())
}

pub fn is_running<G: ContainerGame>(
    paths: &Paths,
    db: &Db,
    instance: &G::Instance,
) -> Result<bool> {
    let identity = G::identity(db, instance)?;
    if !container_instances::is_podman(db, &identity.id)? {
        return G::running_directly(instance);
    }
    let Some(config) = GlobalConfig::load(paths)?.podman else {
        return Ok(false);
    };
    let client = PodmanClient::from_config(&config)?;
    Ok(client
        .inspect_blocking(&container_name(G::GAME, &identity.id))?
        .is_some_and(ContainerState::is_running))
}

pub async fn is_running_async<G: ContainerGame>(
    paths: &Paths,
    db: &Db,
    instance: &G::Instance,
) -> Result<bool> {
    let identity = G::identity(db, instance)?;
    if !container_instances::is_podman(db, &identity.id)? {
        return G::running_directly(instance);
    }
    let client = PodmanClient::from_config(&config(paths, G::GAME)?)?;
    Ok(client
        .inspect(&container_name(G::GAME, &identity.id))
        .await?
        .is_some_and(ContainerState::is_running))
}

pub async fn start<G: ContainerGame>(paths: &Paths, db: &Db, name: &str) -> Result<G::Instance> {
    let _lock = LifecycleLock::acquire(paths, G::GAME, name)?;
    let instance = G::load(paths, db, name)?;
    let identity = G::identity(db, &instance)?;
    require_migrated(db, &identity)?;
    crate::game::ports::ensure_available(paths, db, G::GAME, name, G::ports(&instance)?)?;
    G::prepare_layout(paths, &identity, &instance)?;

    let config = config(paths, G::GAME)?;
    let client = PodmanClient::from_config(&config)?;
    client.ensure_supported().await?;
    let limits = resource_limits::load(db, &identity.id)?;
    let container = container_name(G::GAME, &identity.id);
    let image = config.runtime_image();
    let spec = ContainerSpec::new(
        paths,
        &identity,
        &G::instance_dir(paths, &instance),
        G::extra_mounts(paths),
        image.clone(),
        &limits,
    );
    match client.inspect(&container).await? {
        Some(state) if state.is_running() => bail!(InstanceError::AlreadyRunning(name.to_string())),
        Some(_) => {
            client.update_limits(&container, &limits).await?;
            client.start(&container).await?;
        }
        None => {
            client.ensure_image(&image).await?;
            client.create(&spec).await?;
            wait_for_container(&client, &container).await?;
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
    container_instances::set_desired_running(db, &identity.id, true)?;
    Ok(instance)
}

async fn wait_for_container(client: &PodmanClient, name: &str) -> Result<()> {
    let deadline = Instant::now() + CONTAINER_CREATE_TIMEOUT;
    loop {
        if client.inspect(name).await?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "Podman created container {name} but it did not become available within {} seconds",
                CONTAINER_CREATE_TIMEOUT.as_secs()
            );
        }
        sleep(CONTAINER_CREATE_POLL_INTERVAL).await;
    }
}

pub async fn stop<G: ContainerGame>(
    paths: &Paths,
    db: &Db,
    name: &str,
    reason: StopReason,
) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, G::GAME, name)?;
    let instance = G::load(paths, db, name)?;
    let identity = G::identity(db, &instance)?;
    require_migrated(db, &identity)?;
    // Recorded before anything is signalled so a container that is going down
    // can never be mistaken for one that crashed.
    if reason.clears_desired_running() {
        container_instances::set_desired_running(db, &identity.id, false)?;
    }
    let config = config(paths, G::GAME)?;
    let client = PodmanClient::from_config(&config)?;
    let container = container_name(G::GAME, &identity.id);
    let timeout_secs = game::driver(G::GAME).stop_timeout_secs();
    let state = client.inspect(&container).await?;
    let Some(state) = state else {
        bail!(InstanceError::NotRunning(name.to_string()));
    };
    match state {
        ContainerState::Created | ContainerState::Exited => {
            bail!(InstanceError::NotRunning(name.to_string()));
        }
        ContainerState::Running | ContainerState::Unknown => {
            let socket = control_socket(paths, &identity);
            if supervisor::client::stop_socket(&socket, timeout_secs)
                .await
                .is_ok()
                && wait_for_container_stop(
                    &client,
                    &container,
                    Duration::from_secs(timeout_secs) + STOP_GRACE,
                )
                .await
                .is_ok()
            {
                return Ok(());
            }
            client.stop(&container, timeout_secs).await?;
        }
    }
    Ok(())
}

async fn wait_for_container_stop(
    client: &PodmanClient,
    name: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match client.inspect(name).await? {
            None | Some(ContainerState::Created | ContainerState::Exited) => return Ok(()),
            Some(ContainerState::Running | ContainerState::Unknown) => {}
        }
        if Instant::now() >= deadline {
            bail!(
                "Podman container {name} did not stop within {} seconds",
                timeout.as_secs()
            );
        }
        sleep(STOP_POLL_INTERVAL).await;
    }
}

pub async fn restart<G: ContainerGame>(paths: &Paths, db: &Db, name: &str) -> Result<G::Instance> {
    let instance = G::load(paths, db, name)?;
    if is_running_async::<G>(paths, db, &instance).await? {
        stop::<G>(paths, db, name, StopReason::Restart).await?;
    }
    start::<G>(paths, db, name).await
}

/// Updates the game payload inside an already-running runtime. The container
/// remains alive; the runtime stops and restarts only its game child.
pub async fn update<G: ContainerGame>(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let instance = G::load(paths, db, name)?;
    let identity = G::identity(db, &instance)?;
    require_migrated(db, &identity)?;
    if !is_running_async::<G>(paths, db, &instance).await? {
        bail!(
            "{} update requires a running instance; Odin will not start it implicitly",
            display(G::GAME)
        );
    }
    supervisor::client::update_socket(&control_socket(paths, &identity)).await
}

pub async fn update_limits(paths: &Paths, db: &Db, game: GameId, instance_id: &str) -> Result<()> {
    if !container_instances::is_podman(db, instance_id)? {
        return Ok(());
    }
    let config = config(paths, game)?;
    let client = PodmanClient::from_config(&config)?;
    let container = container_name(game, instance_id);
    if client.inspect(&container).await?.is_none() {
        return Ok(());
    }
    let limits = resource_limits::load(db, instance_id)?;
    client.update_limits(&container, &limits).await
}

pub fn remove<G: ContainerGame>(paths: &Paths, db: &Db, instance: &G::Instance) -> Result<()> {
    let identity = G::identity(db, instance)?;
    if !container_instances::is_podman(db, &identity.id)? {
        return Ok(());
    }
    // Without a configured socket Odin cannot have created a container.
    if let Some(config) = GlobalConfig::load(paths)?.podman {
        PodmanClient::from_config(&config)?
            .remove_blocking(&container_name(G::GAME, &identity.id), true)?;
    }
    // The private runtime volume (SteamCMD and the game payload) belongs to
    // this instance alone and would otherwise outlive it.
    for directory in [
        paths.container_runtime_dir(G::GAME, &identity.id),
        paths.container_socket_dir(&identity.id),
    ] {
        match std::fs::remove_dir_all(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to remove {}", directory.display()));
            }
        }
    }
    Ok(())
}

fn unavailable_container() -> RuntimeContainerHealth {
    RuntimeContainerHealth {
        status: "unavailable",
        error: None,
    }
}

async fn socket_health(
    control_socket: &Path,
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

fn socket_dir_has_socket(socket_dir: &Path) -> bool {
    std::fs::read_dir(socket_dir).ok().is_some_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_type()
                .map(|file_type| file_type.is_socket())
                .unwrap_or(false)
        })
    })
}

fn clean_socket_dir(socket_dir: &Path) -> Result<()> {
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

pub fn control_socket(paths: &Paths, identity: &GameInstanceIdentity) -> PathBuf {
    paths
        .container_socket_dir(&identity.id)
        .join(format!("{}-{}.control.sock", identity.game, identity.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

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
    async fn wait_for_container_retries_until_podman_exposes_it() {
        let socket =
            std::env::temp_dir().join(format!("odin-podman-api-{}.sock", uuid::Uuid::new_v4()));
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in [
                ("404 Not Found", r#"{"cause":"no such container"}"#),
                (
                    "200 OK",
                    r#"{"State":{"Running":false,"Status":"created"}}"#,
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let response = format!(
                    "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                requests.push(request);
            }
            requests
        });

        wait_for_container(
            &PodmanClient::new(&socket).unwrap(),
            "odin-valheim-instance",
        )
        .await
        .unwrap();

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| {
            request.starts_with("GET /v5.0.0/libpod/containers/odin-valheim-instance/json ")
        }));
        std::fs::remove_file(socket).ok();
    }

    #[tokio::test]
    async fn wait_for_container_stop_waits_for_the_runtime_to_exit() {
        let socket =
            std::env::temp_dir().join(format!("odin-podman-api-{}.sock", uuid::Uuid::new_v4()));
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            for body in [
                r#"{"State":{"Running":true,"Status":"running"}}"#,
                r#"{"State":{"Running":false,"Status":"exited"}}"#,
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });

        wait_for_container_stop(
            &PodmanClient::new(&socket).unwrap(),
            "odin-valheim-instance",
            Duration::from_secs(35),
        )
        .await
        .unwrap();

        server.join().unwrap();
        std::fs::remove_file(socket).ok();
    }
}
