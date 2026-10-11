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
use crate::instance::Instance;
use crate::paths::Paths;

pub mod runtime;
pub mod valheim;

/// Podman keeps the Libpod API backwards compatible within a major release.
/// v4.0.0 is the oldest server Odin supports, so this path works both there
/// and on newer Podman releases without tying the client to their API level.
const API_PREFIX: &str = "http://d/v4.0.0/libpod";
const RUNTIME_VERSION: &str = env!("CARGO_PKG_VERSION");
const CONTAINER_ROOT: &str = "/srv/odin";

#[derive(Clone)]
pub struct PodmanClient {
    client: Client,
    socket: PathBuf,
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
                "Podman {} is unsupported; Odin requires Podman 4.0.0 or newer",
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

    pub async fn pull_image(&self, image: &str) -> Result<()> {
        let response = self
            .client
            .post(format!("{API_PREFIX}/images/pull?reference={image}"))
            .send()
            .await
            .context("failed to pull Podman runtime image")?;
        success(response).await?;
        Ok(())
    }

    pub async fn ensure_image(&self, image: &str) -> Result<()> {
        if self.image_exists(image).await? {
            return Ok(());
        }
        self.pull_image(image)
            .await
            .with_context(|| format!("failed to pull runtime image {image}"))
    }

    pub async fn inspect(&self, name: &str) -> Result<Option<ContainerState>> {
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
        Ok(Some(ContainerState::from_inspect(inspect)))
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
        Ok(Some(ContainerState::from_inspect(inspect)))
    }

    pub async fn create(&self, spec: &ContainerSpec) -> Result<()> {
        let response = self
            .client
            .post(format!("{API_PREFIX}/containers/create?name={}", spec.name))
            .json(&spec.request)
            .send()
            .await
            .context("failed to create Podman container")?;
        success(response).await?;
        Ok(())
    }

    pub async fn start(&self, name: &str) -> Result<()> {
        let response = self
            .client
            .post(format!("{API_PREFIX}/containers/{name}/start"))
            .send()
            .await
            .context("failed to start Podman container")?;
        success(response).await?;
        Ok(())
    }

    pub async fn stop(&self, name: &str, timeout_secs: u64) -> Result<()> {
        let response = self
            .client
            .post(format!(
                "{API_PREFIX}/containers/{name}/stop?t={timeout_secs}"
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

    pub async fn update_limits(&self, name: &str, limits: &ResourceLimits) -> Result<()> {
        limits.validate()?;
        let request = PodmanResources::from(limits);
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
            .is_some_and(|major| major >= 4)
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

    fn from_inspect(response: InspectResponse) -> Self {
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
}

#[derive(Deserialize)]
struct InspectState {
    #[serde(rename = "Running", default)]
    running: bool,
    #[serde(rename = "Status")]
    status: Option<String>,
}

/// Data written by the host and consumed exclusively by `odin run` inside
/// the container. It carries Odin's typed launch settings, not a second game
/// configuration format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSpec {
    pub protocol_version: u16,
    pub instance_id: String,
    pub instance: crate::instance::state::InstanceState,
    pub instance_dir: PathBuf,
    pub install_dir: PathBuf,
    pub steamcmd_dir: PathBuf,
    pub update_log: PathBuf,
}

impl RuntimeSpec {
    pub const PROTOCOL_VERSION: u16 = 1;

    fn for_instance(identity: &GameInstanceIdentity, instance: &Instance) -> Self {
        Self {
            protocol_version: Self::PROTOCOL_VERSION,
            instance_id: identity.id.clone(),
            instance: instance.state.clone(),
            instance_dir: PathBuf::from(format!("{CONTAINER_ROOT}/instance")),
            install_dir: PathBuf::from(format!("{CONTAINER_ROOT}/runtime/valheim")),
            steamcmd_dir: PathBuf::from(format!("{CONTAINER_ROOT}/runtime/steamcmd")),
            update_log: PathBuf::from(format!("{CONTAINER_ROOT}/instance/logs/update.log")),
        }
    }
}

pub fn runtime_spec_path(paths: &Paths, instance_id: &str) -> PathBuf {
    paths.valheim_runtime_dir(instance_id).join("runtime.json")
}

pub fn write_runtime_spec(
    paths: &Paths,
    identity: &GameInstanceIdentity,
    instance: &Instance,
) -> Result<PathBuf> {
    let runtime_dir = paths.valheim_runtime_dir(&identity.id);
    std::fs::create_dir_all(&runtime_dir)
        .with_context(|| format!("failed to create {}", runtime_dir.display()))?;
    let spec_path = runtime_spec_path(paths, &identity.id);
    let temporary = spec_path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let encoded = serde_json::to_vec_pretty(&RuntimeSpec::for_instance(identity, instance))
        .context("failed to encode Valheim runtime specification")?;
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
    pub name: String,
    pub request: CreateContainerRequest,
}

impl ContainerSpec {
    pub fn valheim(
        paths: &Paths,
        identity: &GameInstanceIdentity,
        instance: &Instance,
        image: String,
        limits: &ResourceLimits,
    ) -> Self {
        let runtime_root = paths.valheim_runtime_dir(&identity.id);
        let socket_root = paths.valheim_socket_dir(&identity.id);
        let mut binds = vec![
            bind(&runtime_root, Path::new("/srv/odin/runtime")),
            bind(&instance.dir, Path::new("/srv/odin/instance")),
            bind(&socket_root, Path::new("/run/odin")),
        ];
        // Existing BepInEx plugin links point at this host-absolute store.
        // Mounting it at that exact path preserves those links without
        // rewriting an operator's mod state.
        binds.push(bind(&paths.mods_dir(), &paths.mods_dir()));
        let user = format!("{}:{}", unsafe { libc::geteuid() }, unsafe {
            libc::getegid()
        });
        let mut labels = std::collections::BTreeMap::new();
        labels.insert("io.odin.game".to_string(), "valheim".to_string());
        labels.insert("io.odin.instance-id".to_string(), identity.id.clone());
        labels.insert(
            "io.odin.runtime-version".to_string(),
            RUNTIME_VERSION.to_string(),
        );

        Self {
            name: container_name(&identity.id),
            request: CreateContainerRequest {
                image,
                entrypoint: vec!["/usr/local/bin/odin".to_string()],
                command: vec![
                    "run".to_string(),
                    "--instance-id".to_string(),
                    identity.id.clone(),
                ],
                env: vec![
                    "ODIN_RUNTIME_SPEC=/srv/odin/runtime/runtime.json".to_string(),
                    "ODIN_RUNTIME_DIR=/run/odin".to_string(),
                    "HOME=/srv/odin/runtime".to_string(),
                ],
                labels,
                user,
                host_config: HostConfig {
                    network_mode: "host".to_string(),
                    binds,
                    cpu_period: Some(100_000),
                    cpu_quota: limits.cpu_percent.map(cpu_quota),
                    memory: limits.memory_max_bytes.map(|value| value as i64),
                },
            },
        }
    }
}

fn bind(host: &Path, container: &Path) -> String {
    format!("{}:{}:rw", host.display(), container.display())
}

pub fn container_name(instance_id: &str) -> String {
    format!("odin-valheim-{instance_id}")
}

fn cpu_quota(percent: f64) -> i64 {
    (percent * 1000.0).round().max(1.0) as i64
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateContainerRequest {
    pub image: String,
    pub entrypoint: Vec<String>,
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub labels: std::collections::BTreeMap<String, String>,
    pub user: String,
    pub host_config: HostConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostConfig {
    pub network_mode: String,
    pub binds: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_period: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_quota: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PodmanResources {
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory: Option<i64>,
}

impl From<&ResourceLimits> for PodmanResources {
    fn from(limits: &ResourceLimits) -> Self {
        Self {
            cpu: limits.cpu_percent.map(cpu_quota),
            memory: limits.memory_max_bytes.map(|value| value as i64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixListener;

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
        let spec = ContainerSpec::valheim(
            &paths,
            &identity,
            &instance,
            "example/runtime:1".into(),
            &ResourceLimits {
                cpu_percent: Some(125.5),
                memory_max_bytes: Some(123),
            },
        );
        assert_eq!(spec.name, container_name(&identity.id));
        assert_eq!(spec.request.entrypoint, ["/usr/local/bin/odin"]);
        assert_eq!(
            spec.request.command,
            ["run", "--instance-id", identity.id.as_str()]
        );
        assert_eq!(spec.request.host_config.network_mode, "host");
        assert!(
            spec.request
                .host_config
                .binds
                .iter()
                .any(|bind| bind.ends_with(":/run/odin:rw"))
        );
        assert!(
            spec.request
                .host_config
                .binds
                .iter()
                .any(|bind| bind.ends_with(":/srv/odin/runtime:rw"))
        );
        assert_eq!(spec.request.host_config.cpu_quota, Some(125_500));
        assert_eq!(spec.request.host_config.memory, Some(123));
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
        let path = write_runtime_spec(&paths, &identity, &instance).unwrap();
        let spec: RuntimeSpec = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(spec.instance_id, identity.id);
        assert_eq!(spec.install_dir, PathBuf::from("/srv/odin/runtime/valheim"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn client_sends_create_request_over_the_configured_unix_socket() {
        let socket = std::env::current_dir()
            .unwrap()
            .join(format!("odin-podman-api-{}.sock", uuid::Uuid::new_v4()));
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
        let spec = ContainerSpec::valheim(
            &paths,
            &identity,
            &instance,
            "example/odin-srv:1".into(),
            &ResourceLimits::default(),
        );
        PodmanClient::new(&socket)
            .unwrap()
            .create(&spec)
            .await
            .unwrap();
        let (line, body) = server.join().unwrap();
        assert!(line.starts_with(&format!(
            "POST /v4.0.0/libpod/containers/create?name={} ",
            spec.name
        )));
        assert!(body.contains("\"entrypoint\":[\"/usr/local/bin/odin\"]"));
        assert!(body.contains("\"network_mode\":\"host\""));
        std::fs::remove_file(socket).ok();
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn inspect_state_maps_podman_running_and_exited_statuses() {
        assert_eq!(
            ContainerState::from_inspect(InspectResponse {
                state: InspectState {
                    running: true,
                    status: Some("running".into())
                },
            }),
            ContainerState::Running
        );
        assert_eq!(
            ContainerState::from_inspect(InspectResponse {
                state: InspectState {
                    running: false,
                    status: Some("exited".into())
                },
            }),
            ContainerState::Exited
        );
    }
}
