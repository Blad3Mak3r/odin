//! Small, deliberately scoped Podman API client and Valheim container shape.
//!
//! Podman exposes its REST API over a Unix socket. Keeping this boundary in
//! Odin avoids shelling out to a second command parser and keeps all callers
//! (the dashboard and legacy CLI) on the same lifecycle implementation.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};

use crate::config::PodmanConfig;
use crate::db::game_instances::GameInstanceIdentity;
use crate::db::resource_limits::ResourceLimits;
use crate::game::GameId;
use crate::game::ark_asa::ArkLaunch;
use crate::game::palworld::PalworldLaunch;
use crate::game::rust::RustLaunch;
use crate::game::seven_days_to_die::SevenDaysLaunch;
use crate::instance::state::InstanceState;
use crate::paths::Paths;

pub mod ark_asa;
pub mod dispatch;
pub mod lifecycle;
pub mod palworld;
pub mod runtime;
pub mod rust;
pub mod seven_days_to_die;
#[cfg(test)]
pub(crate) mod test_support;
pub mod valheim;

/// Podman 5.0.0 is the oldest server Odin supports. Its Libpod API is
/// backwards compatible within the major release, so this versioned path also
/// works on newer Podman releases without tying the client to their API level.
const API_PREFIX: &str = "http://d/v5.0.0/libpod";
const RUNTIME_VERSION: &str = env!("CARGO_PKG_VERSION");
const CONTAINER_ROOT: &str = "/srv/odin";
const CPU_PERIOD: u64 = 100_000;

#[derive(Clone)]
pub struct PodmanClient {
    client: Client,
    socket: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PodmanImage {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "RepoDigests", default)]
    pub repo_digests: Vec<String>,
    #[serde(rename = "Created", default, deserialize_with = "unix_seconds")]
    pub created: i64,
}

/// Podman reports an image's creation time as an RFC 3339 string (Libpod
/// 5.x) or as Unix seconds (older and Docker-compatible endpoints).
fn unix_seconds<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Created {
        Seconds(i64),
        Text(String),
    }
    match Created::deserialize(deserializer)? {
        Created::Seconds(seconds) => Ok(seconds),
        Created::Text(text) => chrono::DateTime::parse_from_rfc3339(&text)
            .map(|time| time.timestamp())
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Clone)]
pub struct PodmanContainer {
    pub state: ContainerState,
    pub image_id: Option<String>,
}

impl PodmanClient {
    pub fn new(socket: &Path) -> Result<Self> {
        if !socket.is_absolute() {
            bail!("podman.socket must be an absolute Unix socket path");
        }
        let client = Client::builder()
            .unix_socket(socket)
            .build()
            .context("failed to create Podman Unix-socket client")?;
        Ok(Self {
            client,
            socket: socket.to_path_buf(),
        })
    }

    pub fn from_config(config: &PodmanConfig) -> Result<Self> {
        Self::new(&config.socket)
    }

    pub async fn version(&self) -> Result<PodmanVersion> {
        let response = self
            .client
            // The client is pinned to a Unix socket, so this is an HTTP
            // request target, not an unencrypted network connection.
            .get("http://d/version")
            .send()
            .await
            .context("failed to contact Podman version API")?;
        success(response)
            .await?
            .json()
            .await
            .context("failed to decode Podman version response")
    }

    pub async fn ensure_supported(&self) -> Result<()> {
        let version = self.version().await?;
        if version.is_supported() {
            Ok(())
        } else {
            bail!(
                "Podman {} is unsupported; Odin requires Podman 5.0.0 or newer",
                version.version
            )
        }
    }

    pub async fn image_exists(&self, image: &str) -> Result<bool> {
        let response = self
            .client
            .get(format!("{API_PREFIX}/images/{image}/exists"))
            .send()
            .await;
        match response {
            Ok(response) if response.status().is_success() => Ok(true),
            Ok(response) if response.status() == StatusCode::NOT_FOUND => Ok(false),
            Ok(response) => Err(status_error(response).await),
            Err(error) => Err(error).context("failed to contact Podman image API"),
        }
    }

    pub async fn inspect_image(&self, image: &str) -> Result<Option<PodmanImage>> {
        let response = self
            .client
            .get(format!("{API_PREFIX}/images/{image}/json"))
            .send()
            .await
            .context("failed to inspect Podman image")?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        success(response)
            .await?
            .json()
            .await
            .map(Some)
            .context("failed to decode Podman image inspection")
    }

    pub async fn pull_image(&self, image: &str) -> Result<()> {
        let response = self
            .client
            .post(format!(
                "{API_PREFIX}/images/pull?reference={image}&quiet=true"
            ))
            .send()
            .await
            .context("failed to pull Podman runtime image")?;
        let pull: PullImageResponse = success(response)
            .await?
            .json()
            .await
            .context("failed to decode Podman image pull response")?;
        if let Some(error) = pull.error {
            bail!("Podman failed to pull image {image}: {error}");
        }
        Ok(())
    }

    pub fn pull_image_blocking(&self, image: &str) -> Result<()> {
        let response = self
            .blocking_client()?
            .post(format!(
                "{API_PREFIX}/images/pull?reference={image}&quiet=true"
            ))
            .send()
            .context("failed to pull Podman runtime image")?;
        let pull: PullImageResponse = blocking_success(response)?
            .json()
            .context("failed to decode Podman image pull response")?;
        if let Some(error) = pull.error {
            bail!("Podman failed to pull image {image}: {error}");
        }
        Ok(())
    }

    pub async fn ensure_image(&self, image: &str) -> Result<()> {
        if self.image_exists(image).await? {
            return Ok(());
        }
        self.pull_image(image)
            .await
            .with_context(|| format!("failed to pull runtime image {image}"))?;
        anyhow::ensure!(
            self.image_exists(image).await?,
            "Podman completed the pull but image {image} is unavailable"
        );
        Ok(())
    }

    pub fn ensure_supported_blocking(&self) -> Result<()> {
        let response = self
            .blocking_client()?
            .get("http://d/version")
            .send()
            .context("failed to contact Podman version API")?;
        let version: PodmanVersion = blocking_success(response)?
            .json()
            .context("failed to decode Podman version response")?;
        if version.is_supported() {
            Ok(())
        } else {
            bail!(
                "Podman {} is unsupported; Odin requires Podman 5.0.0 or newer",
                version.version
            )
        }
    }

    pub fn ensure_image_blocking(&self, image: &str) -> Result<()> {
        let response = self
            .blocking_client()?
            .get(format!("{API_PREFIX}/images/{image}/exists"))
            .send()
            .context("failed to contact Podman image API")?;
        if response.status().is_success() {
            return Ok(());
        }
        if response.status() != StatusCode::NOT_FOUND {
            return Err(blocking_status_error(response));
        }
        self.pull_image_blocking(image)
    }

    pub async fn inspect(&self, name: &str) -> Result<Option<ContainerState>> {
        Ok(self
            .inspect_container(name)
            .await?
            .map(|container| container.state))
    }

    pub async fn inspect_container(&self, name: &str) -> Result<Option<PodmanContainer>> {
        let response = self
            .client
            .get(format!("{API_PREFIX}/containers/{name}/json"))
            .send()
            .await
            .context("failed to inspect Podman container")?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let inspect: InspectResponse = success(response)
            .await?
            .json()
            .await
            .context("failed to decode Podman inspect response")?;
        Ok(Some(PodmanContainer {
            state: ContainerState::from_inspect(&inspect),
            image_id: inspect.image,
        }))
    }

    /// Synchronous status probe for legacy CLI paths and the telemetry worker,
    /// both of which already run outside Tokio's request workers. Lifecycle
    /// and dashboard mutations use the async methods above.
    pub fn inspect_blocking(&self, name: &str) -> Result<Option<ContainerState>> {
        let response = self
            .blocking_client()?
            .get(format!("{API_PREFIX}/containers/{name}/json"))
            .send()
            .context("failed to inspect Podman container")?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let inspect: InspectResponse = blocking_success(response)?
            .json()
            .context("failed to decode Podman inspect response")?;
        Ok(Some(ContainerState::from_inspect(&inspect)))
    }

    pub async fn create(&self, spec: &ContainerSpec) -> Result<()> {
        let response = self
            .client
            .post(format!("{API_PREFIX}/containers/create"))
            .json(&spec.request)
            .send()
            .await
            .context("failed to create Podman container")?;
        success(response)
            .await?
            .bytes()
            .await
            .context("failed to read Podman container create response")?;
        Ok(())
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        let response = self
            .client
            .post(format!("{API_PREFIX}/containers/{name}/start"))
            .send()
            .await
            .context("failed to start Podman container")?;
        // Libpod answers 304 when the container is already running.
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(());
        }
        success(response).await?;
        Ok(())
    }

    pub async fn stop(&self, name: &str, timeout_secs: u64) -> Result<()> {
        let response = self
            .client
            .post(format!(
                "{API_PREFIX}/containers/{name}/stop?timeout={timeout_secs}&ignore=true"
            ))
            .send()
            .await
            .context("failed to stop Podman container")?;
        success(response).await?;
        Ok(())
    }

    pub fn remove_blocking(&self, name: &str, force: bool) -> Result<()> {
        let response = self
            .blocking_client()?
            .delete(format!("{API_PREFIX}/containers/{name}?force={force}"))
            .send()
            .context("failed to remove Podman container")?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        blocking_success(response)?;
        Ok(())
    }

    pub async fn remove(&self, name: &str, force: bool) -> Result<()> {
        let response = self
            .client
            .delete(format!("{API_PREFIX}/containers/{name}?force={force}"))
            .send()
            .await
            .context("failed to remove Podman container")?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        success(response).await?;
        Ok(())
    }

    pub fn create_blocking(&self, spec: &ContainerSpec) -> Result<()> {
        let response = self
            .blocking_client()?
            .post(format!("{API_PREFIX}/containers/create"))
            .json(&spec.request)
            .send()
            .context("failed to create Podman container")?;
        blocking_success(response)?
            .bytes()
            .context("failed to read Podman container create response")?;
        Ok(())
    }

    pub fn start_blocking(&self, name: &str) -> Result<()> {
        let response = self
            .blocking_client()?
            .post(format!("{API_PREFIX}/containers/{name}/start"))
            .send()
            .context("failed to start Podman container")?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(());
        }
        blocking_success(response)?;
        Ok(())
    }

    pub fn wait_blocking(&self, name: &str) -> Result<i64> {
        #[derive(Deserialize)]
        struct WaitResponse {
            #[serde(rename = "StatusCode")]
            status_code: i64,
        }
        // The container may run for as long as SteamCMD needs, so the
        // default 30 s request timeout of the blocking client must not apply.
        let client = reqwest::blocking::Client::builder()
            .unix_socket(self.socket.as_path())
            .timeout(None)
            .build()
            .context("failed to create blocking Podman Unix-socket client")?;
        let response = client
            .post(format!("{API_PREFIX}/containers/{name}/wait"))
            .send()
            .context("failed to wait for Podman container")?;
        Ok(blocking_success(response)?
            .json::<WaitResponse>()
            .context("failed to decode Podman container wait response")?
            .status_code)
    }

    pub async fn update_limits(&self, name: &str, limits: &ResourceLimits) -> Result<()> {
        limits.validate()?;
        let request = LinuxResources::from(limits);
        let response = self
            .client
            .post(format!("{API_PREFIX}/containers/{name}/update"))
            .json(&request)
            .send()
            .await
            .context("failed to update Podman resource limits")?;
        success(response).await?;
        Ok(())
    }

    fn blocking_client(&self) -> Result<reqwest::blocking::Client> {
        reqwest::blocking::Client::builder()
            .unix_socket(self.socket.as_path())
            .build()
            .context("failed to create blocking Podman Unix-socket client")
    }
}

async fn success(response: Response) -> Result<Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(status_error(response).await)
    }
}

async fn status_error(response: Response) -> anyhow::Error {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    anyhow::anyhow!("Podman API returned {status}: {body}")
}

fn blocking_success(response: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        Err(anyhow::anyhow!("Podman API returned {status}: {body}"))
    }
}

fn blocking_status_error(response: reqwest::blocking::Response) -> anyhow::Error {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    anyhow::anyhow!("Podman API returned {status}: {body}")
}

#[derive(Debug, Clone, Deserialize)]
pub struct PodmanVersion {
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "ApiVersion")]
    pub api_version: String,
}

impl PodmanVersion {
    pub fn is_supported(&self) -> bool {
        self.version
            .split('.')
            .next()
            .and_then(|major| major.parse::<u64>().ok())
            .is_some_and(|major| major >= 5)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Running,
    Created,
    Exited,
    Unknown,
}

impl ContainerState {
    pub fn is_running(self) -> bool {
        self == Self::Running
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Created => "created",
            Self::Exited => "exited",
            Self::Unknown => "unknown",
        }
    }

    fn from_inspect(response: &InspectResponse) -> Self {
        if response.state.running {
            return Self::Running;
        }
        match response.state.status.as_deref() {
            Some("created") | Some("configured") => Self::Created,
            Some("exited") | Some("stopped") | Some("removing") => Self::Exited,
            _ => Self::Unknown,
        }
    }
}

#[derive(Deserialize)]
struct InspectResponse {
    #[serde(rename = "State")]
    state: InspectState,
    #[serde(rename = "Image")]
    image: Option<String>,
}

#[derive(Deserialize)]
struct InspectState {
    #[serde(rename = "Running", default)]
    running: bool,
    #[serde(rename = "Status")]
    status: Option<String>,
}

#[derive(Deserialize)]
struct PullImageResponse {
    error: Option<String>,
}

/// Data written by the host and consumed exclusively by `odin run` inside
/// the container. It carries Odin's typed launch settings, not a second game
/// configuration format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSpec {
    pub protocol_version: u16,
    pub instance_id: String,
    pub instance_dir: PathBuf,
    pub install_dir: PathBuf,
    pub steamcmd_dir: PathBuf,
    pub update_log: PathBuf,
    #[serde(default)]
    pub shared_install: bool,
    #[serde(flatten)]
    pub launch: Launch,
}

/// The per-game part of a [`RuntimeSpec`]: exactly the launch arguments the
/// game's driver needs, tagged by game so the runtime can pick its driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "game", rename_all = "snake_case")]
pub enum Launch {
    Valheim {
        instance: InstanceState,
    },
    Rust(RustLaunch),
    #[serde(rename = "7d2d")]
    SevenDaysToDie(SevenDaysLaunch),
    Palworld(PalworldLaunch),
    #[serde(rename = "ark-asa")]
    ArkSurvivalAscended(ArkLaunch),
}

impl Launch {
    pub fn game(&self) -> GameId {
        match self {
            Self::Valheim { .. } => GameId::Valheim,
            Self::Rust(_) => GameId::Rust,
            Self::SevenDaysToDie(_) => GameId::SevenDaysToDie,
            Self::Palworld(_) => GameId::Palworld,
            Self::ArkSurvivalAscended(_) => GameId::ArkSurvivalAscended,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Valheim { instance } => &instance.name,
            Self::Rust(launch) => &launch.name,
            Self::SevenDaysToDie(launch) => &launch.name,
            Self::Palworld(launch) => &launch.name,
            Self::ArkSurvivalAscended(launch) => &launch.name,
        }
    }

    /// Prepares whatever the game needs inside the runtime volume and builds
    /// the command that starts its server. This is the container side's only
    /// per-game launch hook.
    pub fn server_command(
        &self,
        spec: &RuntimeSpec,
        console_log: &Path,
    ) -> Result<tokio::process::Command> {
        match self {
            Self::Valheim { instance } => crate::game::valheim::build_command_at(
                &crate::instance::Instance {
                    dir: spec.instance_dir.clone(),
                    state: instance.clone(),
                },
                &spec.install_dir,
                &crate::paths::instance_saves_dir(&spec.instance_dir),
                console_log,
            ),
            Self::Rust(launch) => {
                crate::game::rust::prepare_runtime(
                    &spec.install_dir,
                    &spec.instance_dir,
                    &spec.steamcmd_dir,
                    launch,
                )?;
                crate::game::rust::build_command_at(&spec.install_dir, console_log, launch)
            }
            Self::SevenDaysToDie(launch) => {
                crate::game::seven_days_to_die::prepare_runtime(
                    &spec.install_dir,
                    &spec.instance_dir,
                )?;
                crate::game::seven_days_to_die::build_command_at(
                    &spec.install_dir,
                    &spec.instance_dir,
                    console_log,
                    launch,
                )
            }
            Self::Palworld(launch) => {
                crate::game::palworld::prepare_runtime(&spec.install_dir, &spec.instance_dir)?;
                crate::game::palworld::build_command_at(&spec.install_dir, console_log, launch)
            }
            Self::ArkSurvivalAscended(launch) => crate::game::ark_asa::build_command_at(
                &spec.install_dir,
                console_log,
                &spec.steamcmd_dir.join("proton-prefix"),
                &spec.steamcmd_dir,
                launch,
            ),
        }
    }

    /// Asks the running server to save and exit through its own console.
    /// Returns whether the request was delivered; `false` leaves the caller
    /// to signal the process instead.
    pub async fn request_graceful_stop(&self, instance_dir: &Path) -> bool {
        match self {
            // SIGINT ends Rust without saving the world; its console `quit`
            // saves first.
            Self::Rust(launch) => tokio::time::timeout(
                CONSOLE_STOP_TIMEOUT,
                crate::game::rust::rcon::quit(launch.rcon_port, &launch.rcon_password),
            )
            .await
            .is_ok_and(|result| result.is_ok()),
            // The console's `shutdown` saves the world before the server exits.
            Self::SevenDaysToDie(_) => {
                let instance_dir = instance_dir.to_path_buf();
                tokio::time::timeout(
                    CONSOLE_STOP_TIMEOUT,
                    tokio::task::spawn_blocking(move || {
                        crate::game::seven_days_to_die::shutdown(&instance_dir)
                    }),
                )
                .await
                .is_ok_and(|joined| joined.is_ok_and(|result| result.is_ok()))
            }
            // The REST API's `shutdown` saves the world before the server exits.
            Self::Palworld(_) => {
                let instance_dir = instance_dir.to_path_buf();
                tokio::time::timeout(
                    CONSOLE_STOP_TIMEOUT,
                    tokio::task::spawn_blocking(move || {
                        crate::game::palworld::shutdown_at(&instance_dir)
                    }),
                )
                .await
                .is_ok_and(|joined| joined.is_ok_and(|result| result.is_ok()))
            }
            // Valheim saves on SIGINT.
            Self::Valheim { .. } => false,
            Self::ArkSurvivalAscended(launch) => tokio::time::timeout(
                CONSOLE_STOP_TIMEOUT,
                crate::game::ark_asa::save_and_exit(launch.rcon_port, &launch.rcon_password),
            )
            .await
            .is_ok_and(|result| result.is_ok()),
        }
    }
}

const CONSOLE_STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl RuntimeSpec {
    pub const PROTOCOL_VERSION: u16 = 1;

    fn new(instance_id: &str, launch: Launch, install_dir: Option<PathBuf>) -> Self {
        Self {
            protocol_version: Self::PROTOCOL_VERSION,
            instance_id: instance_id.to_string(),
            instance_dir: PathBuf::from(format!("{CONTAINER_ROOT}/instance")),
            install_dir: install_dir.unwrap_or_else(|| {
                PathBuf::from(format!("{CONTAINER_ROOT}/runtime/{}", launch.game()))
            }),
            steamcmd_dir: PathBuf::from(format!("{CONTAINER_ROOT}/runtime/steamcmd")),
            update_log: PathBuf::from(format!("{CONTAINER_ROOT}/instance/logs/update.log")),
            shared_install: false,
            launch,
        }
    }
}

pub fn runtime_spec_path(paths: &Paths, game: GameId, instance_id: &str) -> PathBuf {
    paths
        .container_runtime_dir(game, instance_id)
        .join("runtime.json")
}

pub fn write_runtime_spec(
    paths: &Paths,
    identity: &GameInstanceIdentity,
    launch: Launch,
) -> Result<PathBuf> {
    write_runtime_spec_with_install_dir(paths, identity, launch, None)
}

pub fn write_runtime_spec_with_install_dir(
    paths: &Paths,
    identity: &GameInstanceIdentity,
    launch: Launch,
    install_dir: Option<PathBuf>,
) -> Result<PathBuf> {
    let runtime_dir = paths.container_runtime_dir(identity.game, &identity.id);
    std::fs::create_dir_all(&runtime_dir)
        .with_context(|| format!("failed to create {}", runtime_dir.display()))?;
    let spec_path = runtime_spec_path(paths, identity.game, &identity.id);
    let temporary = spec_path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut spec = RuntimeSpec::new(&identity.id, launch, install_dir.clone());
    spec.shared_install = install_dir.is_some();
    let encoded =
        serde_json::to_vec_pretty(&spec).context("failed to encode runtime specification")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .with_context(|| format!("failed to create {}", temporary.display()))?;
    file.write_all(&encoded)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", temporary.display()))?;
    std::fs::rename(&temporary, &spec_path).with_context(|| {
        format!(
            "failed to atomically activate runtime spec {}",
            spec_path.display()
        )
    })?;
    Ok(spec_path)
}

#[derive(Debug, Clone)]
pub struct ContainerSpec {
    pub request: CreateContainerRequest,
}

impl ContainerSpec {
    /// The container shape shared by every game: a private runtime volume, the
    /// instance directory and the control-socket directory, on the host
    /// network. `extra_mounts` carries anything only one game needs.
    #[cfg(test)]
    pub fn new(
        paths: &Paths,
        identity: &GameInstanceIdentity,
        instance_dir: &Path,
        extra_mounts: Vec<Mount>,
        image: String,
        limits: &ResourceLimits,
    ) -> Self {
        Self::new_with_odin_binary(
            paths,
            identity,
            instance_dir,
            extra_mounts,
            image,
            Path::new("/usr/bin/odin"),
            limits,
        )
    }

    pub fn new_with_odin_binary(
        paths: &Paths,
        identity: &GameInstanceIdentity,
        instance_dir: &Path,
        extra_mounts: Vec<Mount>,
        image: String,
        odin_binary: &Path,
        limits: &ResourceLimits,
    ) -> Self {
        let runtime_root = paths.container_runtime_dir(identity.game, &identity.id);
        let socket_root = paths.container_socket_dir(&identity.id);
        let mut mounts = vec![
            bind(&runtime_root, Path::new("/srv/odin/runtime")),
            bind(instance_dir, Path::new("/srv/odin/instance")),
            bind(&socket_root, Path::new("/run/odin")),
            bind_ro(odin_binary, Path::new("/usr/local/bin/odin")),
        ];
        mounts.extend(extra_mounts);
        let user = format!("{}:{}", unsafe { libc::geteuid() }, unsafe {
            libc::getegid()
        });
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("io.odin.game".to_string(), identity.game.to_string());
        labels.insert("io.odin.instance-id".to_string(), identity.id.clone());
        labels.insert(
            "io.odin.runtime-version".to_string(),
            RUNTIME_VERSION.to_string(),
        );

        let name = container_name(identity.game, &identity.id);
        Self {
            request: CreateContainerRequest {
                name,
                image,
                entrypoint: vec!["/usr/local/bin/odin".to_string()],
                command: vec![
                    "run".to_string(),
                    "--instance-id".to_string(),
                    identity.id.clone(),
                ],
                env: std::collections::BTreeMap::from([
                    (
                        "ODIN_RUNTIME_SPEC".to_string(),
                        "/srv/odin/runtime/runtime.json".to_string(),
                    ),
                    ("ODIN_RUNTIME_DIR".to_string(), "/run/odin".to_string()),
                    ("HOME".to_string(), "/srv/odin/runtime".to_string()),
                ]),
                labels,
                user,
                mounts,
                netns: Namespace {
                    nsmode: "host".to_string(),
                },
                resource_limits: (!limits.is_unlimited()).then(|| LinuxResources::from(limits)),
            },
        }
    }
}

/// Bind mount of `host` at `container`, read-write.
pub fn bind(host: &Path, container: &Path) -> Mount {
    Mount {
        destination: container.display().to_string(),
        kind: "bind".to_string(),
        source: host.display().to_string(),
        options: vec!["rw".to_string()],
    }
}

/// Bind mount of `host` at `container`, read-only.
pub fn bind_ro(host: &Path, container: &Path) -> Mount {
    Mount {
        destination: container.display().to_string(),
        kind: "bind".to_string(),
        source: host.display().to_string(),
        options: vec!["ro".to_string()],
    }
}

pub fn container_name(game: GameId, instance_id: &str) -> String {
    format!("odin-{game}-{instance_id}")
}

fn cpu_quota(percent: f64) -> i64 {
    (percent * CPU_PERIOD as f64 / 100.0).round().max(1.0) as i64
}

/// Body of `POST /libpod/containers/create` (Podman's `specgen.SpecGenerator`).
/// Libpod ignores unknown fields, so only SpecGenerator names belong here; the
/// Docker-compat `HostConfig` shape is silently dropped by this endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct CreateContainerRequest {
    pub name: String,
    pub image: String,
    pub entrypoint: Vec<String>,
    pub command: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub labels: std::collections::BTreeMap<String, String>,
    pub user: String,
    pub mounts: Vec<Mount>,
    pub netns: Namespace,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_limits: Option<LinuxResources>,
}

/// OCI runtime-spec mount.
#[derive(Debug, Clone, Serialize)]
pub struct Mount {
    pub destination: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub source: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Namespace {
    pub nsmode: String,
}

/// OCI runtime-spec `LinuxResources`: the `resource_limits` value of a create
/// request and, unwrapped, the body of `POST /libpod/containers/{name}/update`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LinuxResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<LinuxCpu>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<LinuxMemory>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LinuxCpu {
    pub period: u64,
    pub quota: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LinuxMemory {
    pub limit: i64,
}

impl From<&ResourceLimits> for LinuxResources {
    fn from(limits: &ResourceLimits) -> Self {
        Self {
            cpu: limits.cpu_percent.map(|percent| LinuxCpu {
                period: CPU_PERIOD,
                quota: cpu_quota(percent),
            }),
            memory: limits.memory_max_bytes.map(|value| LinuxMemory {
                limit: value as i64,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::Instance;
    use chrono::Utc;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixListener;

    #[test]
    fn image_created_accepts_rfc3339_and_unix_seconds() {
        let text: PodmanImage =
            serde_json::from_str(r#"{"Id":"a","Created":"2026-10-13T09:55:03.213444578Z"}"#)
                .unwrap();
        assert_eq!(text.created, 1_791_885_303);
        let seconds: PodmanImage = serde_json::from_str(r#"{"Id":"a","Created":42}"#).unwrap();
        assert_eq!(seconds.created, 42);
        let missing: PodmanImage = serde_json::from_str(r#"{"Id":"a"}"#).unwrap();
        assert_eq!(missing.created, 0);
    }

    fn paths() -> Paths {
        let data_dir = std::env::temp_dir().join(format!("odin-podman-{}", uuid::Uuid::new_v4()));
        Paths {
            config_dir: data_dir.clone(),
            data_dir,
        }
    }

    #[test]
    fn valheim_container_uses_uuid_command_host_network_and_isolated_mounts() {
        let paths = paths();
        let identity = GameInstanceIdentity {
            id: "3c1b20a3-5bc3-495a-a8f7-9c6d8e10e0cc".into(),
            game: crate::game::GameId::Valheim,
            name: "renamable".into(),
            created_at: Utc::now(),
            tags: vec![],
        };
        let instance = Instance {
            dir: paths.instance_dir("renamable"),
            state: crate::instance::state::InstanceState::new("renamable", 2456),
        };
        let spec = ContainerSpec::new(
            &paths,
            &identity,
            &instance.dir,
            vec![bind(&paths.mods_dir(), &paths.mods_dir())],
            "example/runtime:1".into(),
            &ResourceLimits {
                cpu_percent: Some(125.5),
                memory_max_bytes: Some(123),
            },
        );
        assert_eq!(
            spec.request.name,
            container_name(GameId::Valheim, &identity.id)
        );
        assert_eq!(spec.request.entrypoint, ["/usr/local/bin/odin"]);
        assert_eq!(
            spec.request.command,
            ["run", "--instance-id", identity.id.as_str()]
        );
        assert_eq!(spec.request.netns.nsmode, "host");
        for destination in ["/run/odin", "/srv/odin/runtime", "/srv/odin/instance"] {
            assert!(
                spec.request
                    .mounts
                    .iter()
                    .any(|mount| mount.destination == destination
                        && mount.kind == "bind"
                        && mount.options == ["rw"]),
                "missing bind mount {destination}"
            );
        }
        assert_eq!(
            spec.request.resource_limits,
            Some(LinuxResources {
                cpu: Some(LinuxCpu {
                    period: 100_000,
                    quota: 125_500
                }),
                memory: Some(LinuxMemory { limit: 123 }),
            })
        );
        assert_eq!(
            spec.request.env.get("ODIN_RUNTIME_SPEC"),
            Some(&"/srv/odin/runtime/runtime.json".to_string())
        );
    }

    fn rust_identity() -> GameInstanceIdentity {
        GameInstanceIdentity {
            id: "0e0f6d1c-7a39-4b5e-8c1a-2d6f5b0a9e11".into(),
            game: GameId::Rust,
            name: "rusty".into(),
            created_at: Utc::now(),
            tags: vec![],
        }
    }

    #[test]
    fn rust_container_has_the_shared_shape_without_valheim_mounts() {
        let paths = paths();
        let identity = rust_identity();
        let instance_dir = paths.game_instance_dir(GameId::Rust, "rusty");

        let request = ContainerSpec::new(
            &paths,
            &identity,
            &instance_dir,
            Vec::new(),
            "example/odin-srv:1".into(),
            &ResourceLimits::default(),
        )
        .request;

        assert_eq!(request.name, format!("odin-rust-{}", identity.id));
        assert_eq!(request.entrypoint, ["/usr/local/bin/odin"]);
        assert_eq!(
            request.command,
            ["run", "--instance-id", identity.id.as_str()]
        );
        assert_eq!(request.netns.nsmode, "host");
        assert_eq!(
            request.labels.get("io.odin.game").map(String::as_str),
            Some("rust")
        );
        assert_eq!(
            request
                .mounts
                .iter()
                .map(|mount| (mount.destination.as_str(), mount.source.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    "/srv/odin/runtime",
                    paths
                        .container_runtime_dir(GameId::Rust, &identity.id)
                        .to_str()
                        .unwrap()
                ),
                ("/srv/odin/instance", instance_dir.to_str().unwrap()),
                (
                    "/run/odin",
                    paths.container_socket_dir(&identity.id).to_str().unwrap()
                ),
                ("/usr/local/bin/odin", "/usr/bin/odin"),
            ]
        );
        assert!(request.resource_limits.is_none());
    }

    #[test]
    fn runtime_spec_round_trips_the_rust_launch_with_container_paths() {
        let paths = paths();
        let identity = rust_identity();
        let launch = RustLaunch {
            name: "rusty".into(),
            identity: identity.id.clone(),
            port: 28015,
            query_port: 28016,
            rcon_port: 28017,
            rcon_password: "secret".into(),
        };

        let path = write_runtime_spec(&paths, &identity, Launch::Rust(launch)).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let spec: RuntimeSpec = serde_json::from_value(json.clone()).unwrap();

        assert_eq!(
            path,
            paths
                .container_runtime_dir(GameId::Rust, &identity.id)
                .join("runtime.json")
        );
        assert_eq!(json["game"], "rust");
        assert_eq!(spec.install_dir, PathBuf::from("/srv/odin/runtime/rust"));
        assert_eq!(
            spec.steamcmd_dir,
            PathBuf::from("/srv/odin/runtime/steamcmd")
        );
        assert_eq!(spec.instance_dir, PathBuf::from("/srv/odin/instance"));
        assert!(matches!(
            spec.launch,
            Launch::Rust(launch) if launch.rcon_port == 28017 && launch.rcon_password == "secret"
        ));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn runtime_spec_round_trips_the_7d2d_launch_under_its_game_id() {
        let paths = paths();
        let identity = GameInstanceIdentity {
            game: GameId::SevenDaysToDie,
            name: "undead".into(),
            ..rust_identity()
        };
        let launch = SevenDaysLaunch {
            name: "undead".into(),
            port: 26900,
            telnet_port: Some(26903),
        };

        let path = write_runtime_spec(&paths, &identity, Launch::SevenDaysToDie(launch)).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let spec: RuntimeSpec = serde_json::from_value(json.clone()).unwrap();

        assert_eq!(
            path,
            paths
                .container_runtime_dir(GameId::SevenDaysToDie, &identity.id)
                .join("runtime.json")
        );
        // The tag must be the GameId's own spelling so the runtime directory,
        // the container label and the spec agree.
        assert_eq!(json["game"], "7d2d");
        assert_eq!(spec.install_dir, PathBuf::from("/srv/odin/runtime/7d2d"));
        assert_eq!(spec.launch.game(), GameId::SevenDaysToDie);
        assert!(matches!(
            spec.launch,
            Launch::SevenDaysToDie(launch) if launch.port == 26900 && launch.telnet_port == Some(26903)
        ));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn runtime_spec_round_trips_the_palworld_launch_under_its_game_id() {
        let paths = paths();
        let identity = GameInstanceIdentity {
            game: GameId::Palworld,
            name: "pals".into(),
            ..rust_identity()
        };
        let launch = PalworldLaunch {
            name: "pals".into(),
            port: 8211,
            query_port: 27015,
        };

        let path = write_runtime_spec(&paths, &identity, Launch::Palworld(launch)).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let spec: RuntimeSpec = serde_json::from_value(json.clone()).unwrap();

        assert_eq!(
            path,
            paths
                .container_runtime_dir(GameId::Palworld, &identity.id)
                .join("runtime.json")
        );
        assert_eq!(json["game"], "palworld");
        assert_eq!(
            spec.install_dir,
            PathBuf::from("/srv/odin/runtime/palworld")
        );
        assert_eq!(spec.launch.game(), GameId::Palworld);
        assert!(matches!(
            spec.launch,
            Launch::Palworld(launch) if launch.port == 8211 && launch.query_port == 27015
        ));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn valheim_runtime_spec_keeps_its_instance_key_for_older_runtimes() {
        let state = crate::instance::state::InstanceState::new("one", 2456);
        let spec = RuntimeSpec::new("id", Launch::Valheim { instance: state }, None);

        let json = serde_json::to_value(&spec).unwrap();

        assert_eq!(json["game"], "valheim");
        assert_eq!(json["instance"]["name"], "one");
        assert_eq!(json["install_dir"], "/srv/odin/runtime/valheim");
    }

    #[test]
    fn container_names_are_namespaced_by_game() {
        assert_eq!(
            container_name(GameId::Valheim, "3c1b20a3"),
            "odin-valheim-3c1b20a3"
        );
        assert_eq!(
            container_name(GameId::Rust, "3c1b20a3"),
            "odin-rust-3c1b20a3"
        );
    }

    #[test]
    fn runtime_spec_is_atomic_and_contains_only_runtime_launch_state() {
        let paths = paths();
        let identity = GameInstanceIdentity {
            id: uuid::Uuid::new_v4().to_string(),
            game: crate::game::GameId::Valheim,
            name: "one".into(),
            created_at: Utc::now(),
            tags: vec![],
        };
        let instance = Instance {
            dir: paths.instance_dir("one"),
            state: crate::instance::state::InstanceState::new("one", 2456),
        };
        let path = write_runtime_spec(
            &paths,
            &identity,
            Launch::Valheim {
                instance: instance.state.clone(),
            },
        )
        .unwrap();
        let spec: RuntimeSpec = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(spec.instance_id, identity.id);
        assert_eq!(spec.install_dir, PathBuf::from("/srv/odin/runtime/valheim"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn client_sends_create_request_over_the_configured_unix_socket() {
        let socket =
            std::env::temp_dir().join(format!("odin-podman-api-{}.sock", uuid::Uuid::new_v4()));
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            // Some restricted test sandboxes deny AF_UNIX bind entirely.
            // Keep the test executable there while still exercising the real
            // HTTP-over-UDS request on every normal Linux host and in CI.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut content_length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; content_length];
            reader.read_exact(&mut body).unwrap();
            stream
                .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\n{}")
                .unwrap();
            (request_line, String::from_utf8(body).unwrap())
        });

        let paths = paths();
        let identity = GameInstanceIdentity {
            id: uuid::Uuid::new_v4().to_string(),
            game: crate::game::GameId::Valheim,
            name: "one".into(),
            created_at: Utc::now(),
            tags: vec![],
        };
        let instance = Instance {
            dir: paths.instance_dir("one"),
            state: crate::instance::state::InstanceState::new("one", 2456),
        };
        let spec = ContainerSpec::new(
            &paths,
            &identity,
            &instance.dir,
            Vec::new(),
            "example/odin-srv:1".into(),
            &ResourceLimits::default(),
        );
        PodmanClient::new(&socket)
            .unwrap()
            .create(&spec)
            .await
            .unwrap();
        let (line, body) = server.join().unwrap();
        assert!(line.starts_with("POST /v5.0.0/libpod/containers/create "));
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            body["entrypoint"],
            serde_json::json!(["/usr/local/bin/odin"])
        );
        assert_eq!(body["name"], spec.request.name);
        assert_eq!(body["netns"]["nsmode"], "host");
        assert_eq!(body["mounts"][0]["type"], "bind");
        assert!(body.get("host_config").is_none());
        assert!(body.get("resource_limits").is_none());
        assert_eq!(
            body["env"]["ODIN_RUNTIME_SPEC"],
            "/srv/odin/runtime/runtime.json"
        );
        std::fs::remove_file(socket).ok();
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn client_reports_an_error_embedded_in_an_image_pull_response() {
        let socket =
            std::env::temp_dir().join(format!("odin-podman-api-{}.sock", uuid::Uuid::new_v4()));
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let body = r#"{"error":"manifest unknown"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            request
        });

        let error = PodmanClient::new(&socket)
            .unwrap()
            .pull_image("ghcr.io/example/odin-srv:missing")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("manifest unknown"));
        assert!(server.join().unwrap().starts_with(
            "POST /v5.0.0/libpod/images/pull?reference=ghcr.io/example/odin-srv:missing&quiet=true "
        ));
        std::fs::remove_file(socket).ok();
    }

    #[tokio::test]
    async fn client_removes_a_stopped_container_over_the_configured_unix_socket() {
        let socket =
            std::env::temp_dir().join(format!("odin-podman-api-{}.sock", uuid::Uuid::new_v4()));
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request_line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request_line)
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            request_line
        });

        PodmanClient::new(&socket)
            .unwrap()
            .remove("odin-valheim-instance", false)
            .await
            .unwrap();
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("DELETE /v5.0.0/libpod/containers/odin-valheim-instance?force=false ")
        );
        std::fs::remove_file(socket).ok();
    }

    #[tokio::test]
    async fn client_stops_a_container_with_the_libpod_timeout_parameter() {
        let socket =
            std::env::temp_dir().join(format!("odin-podman-api-{}.sock", uuid::Uuid::new_v4()));
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            request
        });

        PodmanClient::new(&socket)
            .unwrap()
            .stop("odin-valheim-instance", 30)
            .await
            .unwrap();

        assert!(server.join().unwrap().starts_with(
            "POST /v5.0.0/libpod/containers/odin-valheim-instance/stop?timeout=30&ignore=true "
        ));
        std::fs::remove_file(socket).ok();
    }

    #[test]
    fn update_body_is_a_bare_oci_linux_resources_document() {
        let body = serde_json::to_value(LinuxResources::from(&ResourceLimits {
            cpu_percent: Some(50.0),
            memory_max_bytes: Some(1024),
        }))
        .unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "cpu": {"period": 100_000, "quota": 50_000},
                "memory": {"limit": 1024}
            })
        );
        let unlimited = LinuxResources::from(&ResourceLimits::default());
        assert_eq!(
            serde_json::to_value(unlimited).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn only_podman_5_or_newer_is_supported() {
        let version = |version: &str| PodmanVersion {
            version: version.into(),
            api_version: "1.41".into(),
        };
        assert!(!version("4.9.3").is_supported());
        assert!(version("5.0.0").is_supported());
        assert!(version("5.6.2").is_supported());
        assert!(version("6.0.0").is_supported());
    }

    #[test]
    fn inspect_state_maps_podman_running_and_exited_statuses() {
        assert_eq!(
            ContainerState::from_inspect(&InspectResponse {
                state: InspectState {
                    running: true,
                    status: Some("running".into())
                },
                image: None,
            }),
            ContainerState::Running
        );
        assert_eq!(
            ContainerState::from_inspect(&InspectResponse {
                state: InspectState {
                    running: false,
                    status: Some("exited".into())
                },
                image: None,
            }),
            ContainerState::Exited
        );
    }
}
