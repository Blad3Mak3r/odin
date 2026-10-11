//! Podman lifecycle for Valheim. Existing non-migrated instances are kept
//! stopped until an operator explicitly adopts them through `migrate`.

use std::time::Duration;

use anyhow::{Context, Result, bail};

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
    let config = config(paths)?;
    let client = PodmanClient::from_config(&config)?;
    Ok(client
        .inspect(&container_name(&identity.id))?
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
    let limits = resource_limits::load(db, &identity.id)?;
    let container = container_name(&identity.id);
    let image = config.runtime_image();
    let spec = ContainerSpec::valheim(paths, &identity, &instance, image.clone(), &limits);
    let start_name = name.to_string();
    let start_container = container.clone();
    // reqwest's Unix-socket client is blocking. Keep it off axum's async
    // workers while the runtime may pull a large image or wait for Podman.
    tokio::task::spawn_blocking(move || match client.inspect(&start_container)? {
        Some(state) if state.is_running() => bail!(InstanceError::AlreadyRunning(start_name)),
        Some(_) => {
            client.update_limits(&start_container, &limits)?;
            client.start(&start_container)
        }
        None => {
            client.ensure_image(&image)?;
            client.create(&spec)?;
            client.start(&start_container)
        }
    })
    .await
    .context("Podman start task failed")??;
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
    let inspect_client = client.clone();
    let inspect_container = container.clone();
    let state = tokio::task::spawn_blocking(move || inspect_client.inspect(&inspect_container))
        .await
        .context("Podman inspect task failed")??;
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
        tokio::task::spawn_blocking(move || client.stop(&container, STOP_TIMEOUT_SECS))
            .await
            .context("Podman stop task failed")??;
    }
    Ok(())
}

pub async fn restart(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    let instance = Instance::load_existing(paths, db, name)?;
    if is_running(paths, db, &instance)? {
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
    if !is_running(paths, db, &instance)? {
        bail!("Valheim update requires a running instance; Odin will not start it implicitly");
    }
    supervisor::client::update_socket(&control_socket(paths, &identity)).await
}

pub fn update_limits(paths: &Paths, db: &Db, instance_id: &str) -> Result<()> {
    if !container_instances::is_valheim_podman(db, instance_id)? {
        return Ok(());
    }
    let config = config(paths)?;
    let client = PodmanClient::from_config(&config)?;
    if client.inspect(&container_name(instance_id))?.is_none() {
        return Ok(());
    }
    let limits = resource_limits::load(db, instance_id)?;
    client.update_limits(&container_name(instance_id), &limits)
}

pub fn remove(paths: &Paths, db: &Db, instance: &Instance) -> Result<()> {
    let identity = identity(db, instance)?;
    if !container_instances::is_valheim_podman(db, &identity.id)? {
        return Ok(());
    }
    let config = config(paths)?;
    PodmanClient::from_config(&config)?.remove(&container_name(&identity.id), true)
}

fn control_socket(paths: &Paths, identity: &GameInstanceIdentity) -> std::path::PathBuf {
    paths.valheim_socket_dir(&identity.id).join(format!(
        "{}-{}.control.sock",
        GameId::Valheim,
        identity.name
    ))
}
