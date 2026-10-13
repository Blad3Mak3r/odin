//! ARK: Survival Ascended's process-level launch contract.
//!
//! Wildcard does not ship a Linux dedicated-server document.  The Windows
//! executable runs under Proton; game-owned settings remain in its INI files.

use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub const DEFAULT_MAP: &str = "TheIsland_WP";
const RCON_TIMEOUT: Duration = Duration::from_secs(5);
const RCON_AUTH: i32 = 3;
const RCON_AUTH_RESPONSE: i32 = 2;
const RCON_COMMAND: i32 = 2;
const REQUEST_ID: i32 = 1;
const MAX_RCON_PACKET_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArkLaunchOptions {
    pub map: String,
    pub session_name: String,
    #[serde(default)]
    pub mods: Vec<u64>,
}

impl ArkLaunchOptions {
    pub fn defaults(instance_name: &str) -> Self {
        Self {
            map: DEFAULT_MAP.into(),
            session_name: instance_name.into(),
            mods: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.map.is_empty()
                && self.map.len() <= 64
                && self
                    .map
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
            "ARK map must contain 1 to 64 ASCII letters, numbers, or underscores"
        );
        anyhow::ensure!(
            !self.session_name.is_empty()
                && self.session_name.len() <= 128
                && !self.session_name.contains(['?', '"'])
                && !self.session_name.chars().any(char::is_control),
            "ARK session name must contain 1 to 128 printable characters without ? or \""
        );
        anyhow::ensure!(self.mods.len() <= 64, "ARK supports at most 64 mods");
        let mut unique = self.mods.clone();
        unique.sort_unstable();
        unique.dedup();
        anyhow::ensure!(
            unique.len() == self.mods.len(),
            "ARK mod IDs must be unique"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArkLaunch {
    pub name: String,
    pub port: u16,
    pub query_port: u16,
    pub rcon_port: u16,
    pub rcon_password: String,
    #[serde(flatten)]
    pub options: ArkLaunchOptions,
    #[serde(default)]
    pub cluster: Option<ArkClusterLaunch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArkClusterLaunch {
    pub cluster_id: String,
    pub directory: String,
}

impl ArkLaunch {
    pub fn validate(&self) -> Result<()> {
        self.options.validate()?;
        anyhow::ensure!(
            !self.rcon_password.trim().is_empty(),
            "ARK RCON password cannot be empty"
        );
        Ok(())
    }
}

pub fn build_command_at(
    install_dir: &Path,
    console_log: &Path,
    wine_prefix: &Path,
    steam_client_install: &Path,
    launch: &ArkLaunch,
) -> Result<tokio::process::Command> {
    launch.validate()?;
    let binary = install_dir
        .join(crate::game::driver(crate::game::GameId::ArkSurvivalAscended).server_binary());
    if !binary.is_file() {
        bail!(
            "ARK: Survival Ascended is not installed (expected {})",
            binary.display()
        );
    }
    let cwd = binary.parent().context("ARK server binary has no parent")?;
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(console_log)
        .with_context(|| format!("failed to open {}", console_log.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to duplicate ARK log handle")?;
    let mut command = tokio::process::Command::new("proton");
    command
        .current_dir(cwd)
        .env("STEAM_COMPAT_DATA_PATH", wine_prefix)
        .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_client_install)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr))
        .process_group(0)
        .arg("run")
        .arg("ArkAscendedServer.exe")
        .arg(format!(
            "{}?listen?SessionName={}?QueryPort={}?RCONEnabled=True?RCONPort={}?ServerAdminPassword={}",
            launch.options.map, launch.options.session_name, launch.query_port, launch.rcon_port, launch.rcon_password
        ))
        .arg(format!("-Port={}", launch.port))
        .arg("-NoBattlEye")
        .arg("-servergamelog");
    if !launch.options.mods.is_empty() {
        command.arg(format!(
            "-mods={}",
            launch
                .options
                .mods
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if let Some(cluster) = &launch.cluster {
        command.arg(format!("-clusterid={}", cluster.cluster_id));
        command.arg(format!("-ClusterDirOverride={}", cluster.directory));
    }
    Ok(command)
}

/// Asks ASA to save before it exits. Source RCON runs on the host network, so
/// this deliberately connects only to the loopback listener Odin launched.
pub async fn save_and_exit(rcon_port: u16, password: &str) -> Result<()> {
    let mut stream = timeout(RCON_TIMEOUT, TcpStream::connect(("127.0.0.1", rcon_port)))
        .await
        .context("timed out connecting to ARK RCON")?
        .context("failed to connect to ARK RCON")?;
    write_rcon_packet(&mut stream, REQUEST_ID, RCON_AUTH, password).await?;
    let (id, packet_type, _) = read_rcon_packet(&mut stream).await?;
    anyhow::ensure!(
        id == REQUEST_ID && packet_type == RCON_AUTH_RESPONSE,
        "ARK RCON authentication failed"
    );
    write_rcon_packet(&mut stream, REQUEST_ID, RCON_COMMAND, "SaveWorld").await?;
    write_rcon_packet(&mut stream, REQUEST_ID, RCON_COMMAND, "DoExit").await
}

async fn write_rcon_packet(
    stream: &mut TcpStream,
    id: i32,
    packet_type: i32,
    body: &str,
) -> Result<()> {
    let size = 8usize
        .checked_add(body.len())
        .and_then(|value| value.checked_add(2))
        .context("ARK RCON request is too large")?;
    let size = i32::try_from(size).context("ARK RCON request is too large")?;
    timeout(RCON_TIMEOUT, async {
        stream.write_i32_le(size).await?;
        stream.write_i32_le(id).await?;
        stream.write_i32_le(packet_type).await?;
        stream.write_all(body.as_bytes()).await?;
        stream.write_all(&[0, 0]).await?;
        stream.flush().await
    })
    .await
    .context("timed out writing ARK RCON request")?
    .context("failed to write ARK RCON request")?;
    Ok(())
}

async fn read_rcon_packet(stream: &mut TcpStream) -> Result<(i32, i32, String)> {
    let packet = timeout(RCON_TIMEOUT, async {
        let size = stream.read_i32_le().await?;
        anyhow::ensure!(
            size >= 10 && usize::try_from(size).unwrap_or_default() <= MAX_RCON_PACKET_SIZE,
            "ARK RCON sent an invalid packet size"
        );
        let mut payload = vec![0; usize::try_from(size).expect("validated positive packet size")];
        stream.read_exact(&mut payload).await?;
        Ok::<_, anyhow::Error>(payload)
    })
    .await
    .context("timed out reading ARK RCON response")??;
    let id = i32::from_le_bytes(packet[0..4].try_into().expect("packet includes id"));
    let packet_type = i32::from_le_bytes(packet[4..8].try_into().expect("packet includes type"));
    anyhow::ensure!(
        packet[packet.len() - 2..] == [0, 0],
        "ARK RCON response is missing its terminator"
    );
    let body = String::from_utf8(packet[8..packet.len() - 2].to_vec())
        .context("ARK RCON response is not UTF-8")?;
    Ok((id, packet_type, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_options_reject_invalid_server_contract_values() {
        assert!(ArkLaunchOptions::defaults("alpha").validate().is_ok());
        assert!(
            ArkLaunchOptions {
                map: "bad-map".into(),
                ..ArkLaunchOptions::defaults("alpha")
            }
            .validate()
            .is_err()
        );
        assert!(
            ArkLaunchOptions {
                session_name: "bad?name".into(),
                ..ArkLaunchOptions::defaults("alpha")
            }
            .validate()
            .is_err()
        );
        assert!(
            ArkLaunchOptions {
                mods: vec![1, 1],
                ..ArkLaunchOptions::defaults("alpha")
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn proton_command_is_the_complete_server_contract() {
        let root = std::env::temp_dir().join(format!("odin-ark-command-{}", uuid::Uuid::new_v4()));
        let install = root.join("install");
        let steamcmd = root.join("steamcmd");
        let binary = install.join("ShooterGame/Binaries/Win64/ArkAscendedServer.exe");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::create_dir_all(&steamcmd).unwrap();
        fs::write(&binary, "").unwrap();
        let log = root.join("logs/console.log");
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        let launch = ArkLaunch {
            name: "isla".into(),
            port: 7777,
            query_port: 27015,
            rcon_port: 27020,
            rcon_password: "secret".into(),
            options: ArkLaunchOptions {
                map: "TheIsland_WP".into(),
                session_name: "Isla".into(),
                mods: vec![123, 456],
            },
            cluster: None,
        };

        let command = build_command_at(
            &install,
            &log,
            &steamcmd.join("proton-prefix"),
            &steamcmd,
            &launch,
        )
        .unwrap();
        let command = command.as_std();
        assert_eq!(command.get_program(), "proton");
        assert_eq!(command.get_current_dir(), binary.parent());
        assert_eq!(
            command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            [
                "run",
                "ArkAscendedServer.exe",
                "TheIsland_WP?listen?SessionName=Isla?QueryPort=27015?RCONEnabled=True?RCONPort=27020?ServerAdminPassword=secret",
                "-Port=7777",
                "-NoBattlEye",
                "-servergamelog",
                "-mods=123,456",
            ]
        );
        assert_eq!(
            command
                .get_envs()
                .find_map(
                    |(name, value)| (name == "STEAM_COMPAT_DATA_PATH").then_some(value.unwrap())
                )
                .unwrap(),
            steamcmd.join("proton-prefix").as_os_str()
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn source_rcon_packet_has_the_expected_framing() {
        let mut packet = Vec::new();
        packet.extend_from_slice(&16_i32.to_le_bytes());
        packet.extend_from_slice(&REQUEST_ID.to_le_bytes());
        packet.extend_from_slice(&RCON_COMMAND.to_le_bytes());
        packet.extend_from_slice(b"save");
        packet.extend_from_slice(&[0, 0]);
        assert_eq!(packet.len(), 18);
        assert_eq!(&packet[12..], b"save\0\0");
    }
}
