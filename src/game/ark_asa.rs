//! ARK: Survival Ascended's process-level launch contract.
//!
//! Wildcard does not ship a Linux dedicated-server document.  The Windows
//! executable runs under Proton; game-owned settings remain in its INI files.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MAP: &str = "TheIsland_WP";
const STEAM_CLIENT_DLL: &str = "ShooterGame/Binaries/Win64/steamclient64.dll";

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

/// Removes the Steam client DLL shipped in some ASA depots. Other Linux
/// server images remove it before Proton starts; leaving it beside the game
/// executable can prevent the dedicated server from reaching its own startup.
///
/// Shared installations call this only from their serialized installer job,
/// never from an instance that may share the files with a running server.
pub fn prepare_runtime(install_dir: &Path) -> Result<()> {
    let steam_client = install_dir.join(STEAM_CLIENT_DLL);
    match fs::remove_file(&steam_client) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to remove {}", steam_client.display()))
        }
    }
}

pub fn build_command_at(
    install_dir: &Path,
    console_log: &Path,
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
        .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_client_install)
        .env("PROTON_LOG", "1")
        .env(
            "PROTON_LOG_DIR",
            console_log.parent().unwrap_or_else(|| Path::new(".")),
        )
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

/// Asks ASA to save before it exits.
pub async fn save_and_exit(rcon_port: u16, password: &str) -> Result<()> {
    crate::game::rcon::source::send(rcon_port, password, &["SaveWorld", "DoExit"]).await
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

        let command = build_command_at(&install, &log, &steamcmd, &launch).unwrap();
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
        // The image provides STEAM_COMPAT_DATA_PATH and its directory.
        assert!(
            command
                .get_envs()
                .all(|(name, _)| name != "STEAM_COMPAT_DATA_PATH")
        );
        let env = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(env.get("PROTON_LOG").and_then(Option::as_deref), Some("1"));
        assert_eq!(
            env.get("PROTON_LOG_DIR").and_then(Option::as_deref),
            log.parent().map(|path| path.to_str().unwrap())
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn proton_command_adds_cluster_arguments() {
        let root = std::env::temp_dir().join(format!("odin-ark-cluster-{}", uuid::Uuid::new_v4()));
        let install = root.join("install");
        let binary = install.join("ShooterGame/Binaries/Win64/ArkAscendedServer.exe");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, "").unwrap();
        let log = root.join("console.log");
        let launch = ArkLaunch {
            name: "isla".into(),
            port: 7777,
            query_port: 27015,
            rcon_port: 27020,
            rcon_password: "secret".into(),
            options: ArkLaunchOptions::defaults("isla"),
            cluster: Some(ArkClusterLaunch {
                cluster_id: "islands".into(),
                directory: "/srv/odin/clusters/islands".into(),
            }),
        };
        let command = build_command_at(&install, &log, &root, &launch).unwrap();
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(args.contains(&"-clusterid=islands".to_string()));
        assert!(args.contains(&"-ClusterDirOverride=/srv/odin/clusters/islands".to_string()));
    }

    #[test]
    fn prepare_runtime_removes_only_the_depot_steam_client_dll_idempotently() {
        let root = std::env::temp_dir().join(format!("odin-ark-prepare-{}", uuid::Uuid::new_v4()));
        let dll = root.join(STEAM_CLIENT_DLL);
        let saved = root.join("ShooterGame/Saved");
        let ini = saved.join("Config/WindowsServer/GameUserSettings.ini");
        fs::create_dir_all(dll.parent().unwrap()).unwrap();
        fs::create_dir_all(ini.parent().unwrap()).unwrap();
        fs::write(&dll, "depot copy").unwrap();
        fs::write(&ini, "operator settings").unwrap();
        fs::write(saved.join("world.sav"), "operator save").unwrap();

        prepare_runtime(&root).unwrap();
        prepare_runtime(&root).unwrap();

        assert!(!dll.exists());
        assert_eq!(fs::read_to_string(&ini).unwrap(), "operator settings");
        assert_eq!(
            fs::read_to_string(saved.join("world.sav")).unwrap(),
            "operator save"
        );
        fs::remove_dir_all(root).ok();
    }
}
