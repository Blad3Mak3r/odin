//! ARK: Survival Ascended's process-level launch contract.
//!
//! Wildcard does not ship a Linux dedicated-server document.  The Windows
//! executable runs under Proton; game-owned settings remain in its INI files.

use std::fs;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MAP: &str = "TheIsland_WP";
const STEAM_CLIENT_DLL: &str = "ShooterGame/Binaries/Win64/steamclient64.dll";
const GAME_LOG: &str = "ShooterGame/Saved/Logs/ShooterGame.log";

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

/// Makes SteamCMD's Linux Steam client available to Proton, which loads it
/// natively for the Windows server's Steamworks calls, and returns the
/// directory to hand Proton as its Steam client install. A shared instance has
/// no SteamCMD of its own; it uses the one the shared installer left behind.
pub fn link_steam_client(install_dir: &Path, steamcmd_dir: &Path) -> Result<PathBuf> {
    link_steam_client_at(
        install_dir,
        steamcmd_dir,
        &crate::steamcmd::steam_home_dir()?,
    )
}

fn link_steam_client_at(install_dir: &Path, steamcmd_dir: &Path, home: &Path) -> Result<PathBuf> {
    let client = crate::steamcmd::client_dir(install_dir, steamcmd_dir);
    crate::steamcmd::SteamCmd::new(client.clone())
        .ensure_sdk64_client_at(home)
        .context("failed to prepare ARK: Survival Ascended's Steam client")?;
    Ok(client)
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
        // Proton only writes `steam-<SteamGameId>.log` when the id is set.
        .env(
            "SteamGameId",
            crate::game::driver(crate::game::GameId::ArkSurvivalAscended).steam_app_id(),
        )
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

/// Copies ASA's own `ShooterGame.log` into the instance console log.
///
/// The Windows server writes almost nothing to stdout under Proton, but the
/// engine always logs to this file. The runtime calls [`Self::pump`] while the
/// server runs and once more after it exits.
pub struct LogForwarder {
    source: PathBuf,
    console_log: PathBuf,
    inode: Option<u64>,
    offset: u64,
    partial: Vec<u8>,
}

impl LogForwarder {
    /// Created before the server starts: content already in the file belongs
    /// to a previous run and is skipped, and the engine moving the old file
    /// aside at startup is detected as a new inode.
    pub fn new(install_dir: &Path, console_log: &Path) -> Self {
        let source = install_dir.join(GAME_LOG);
        let (inode, offset) = fs::metadata(&source)
            .map(|meta| (Some(meta.ino()), meta.len()))
            .unwrap_or((None, 0));
        Self {
            source,
            console_log: console_log.to_path_buf(),
            inode,
            offset,
            partial: Vec::new(),
        }
    }

    /// Appends the complete lines written since the last call. Best effort:
    /// a transient I/O error just means the next pump retries.
    pub fn pump(&mut self) {
        let _ = self.try_pump();
    }

    fn try_pump(&mut self) -> std::io::Result<()> {
        let mut file = fs::File::open(&self.source)?;
        let meta = file.metadata()?;
        if self.inode != Some(meta.ino()) {
            self.inode = Some(meta.ino());
            self.offset = 0;
            self.partial.clear();
        }
        if meta.len() < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut chunk = Vec::new();
        file.read_to_end(&mut chunk)?;
        self.offset += chunk.len() as u64;
        self.partial.extend_from_slice(&chunk);
        let Some(end) = self.partial.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(());
        };
        let complete: Vec<u8> = self.partial.drain(..=end).collect();
        let mut text = String::new();
        for line in String::from_utf8_lossy(&complete).lines() {
            text.push_str(line.trim_end_matches('\r'));
            text.push('\n');
        }
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.console_log)?
            .write_all(text.as_bytes())
    }
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
    fn a_shared_instance_takes_the_steam_client_from_the_shared_installer() {
        let root = std::env::temp_dir().join(format!("odin-ark-client-{}", uuid::Uuid::new_v4()));
        let install = root.join("shared");
        let shared = install.join(crate::steamcmd::SHARED_STEAMCMD_DIR);
        let own = root.join("runtime/steamcmd");
        let home = root.join("home");
        fs::create_dir_all(shared.join("linux64")).unwrap();
        fs::create_dir_all(&own).unwrap();
        fs::write(shared.join("linux64/steamclient.so"), "client").unwrap();

        let client = link_steam_client_at(&install, &own, &home).unwrap();

        assert_eq!(client, shared);
        assert_eq!(
            fs::read_to_string(home.join(".steam/sdk64/steamclient.so")).unwrap(),
            "client"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_instance_without_a_steam_client_fails_before_proton_starts() {
        let root = std::env::temp_dir().join(format!("odin-ark-client-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("steamcmd")).unwrap();

        let error =
            link_steam_client_at(&root, &root.join("steamcmd"), &root.join("home")).unwrap_err();

        assert!(format!("{error:#}").contains("linux64/steamclient.so"));
        fs::remove_dir_all(root).ok();
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
        assert_eq!(
            env.get("SteamGameId").and_then(Option::as_deref),
            Some("2430930")
        );
        assert_eq!(env.get("PROTON_LOG").and_then(Option::as_deref), Some("1"));
        assert_eq!(
            env.get("PROTON_LOG_DIR").and_then(Option::as_deref),
            log.parent().map(|path| path.to_str().unwrap())
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn log_forwarder_copies_new_complete_lines_of_the_current_run() {
        let root = std::env::temp_dir().join(format!("odin-ark-forward-{}", uuid::Uuid::new_v4()));
        let source = root.join(GAME_LOG);
        let console = root.join("console.log");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "previous run\n").unwrap();
        let mut forwarder = LogForwarder::new(&root, &console);

        // Stale content from the previous run is skipped.
        forwarder.pump();
        assert!(!console.exists());

        // The engine moves the old log aside and starts a new one.
        fs::rename(&source, root.join("backup.log")).unwrap();
        fs::write(&source, "[boot]\r\nhalf a li").unwrap();
        forwarder.pump();
        assert_eq!(fs::read_to_string(&console).unwrap(), "[boot]\n");

        let mut file = fs::OpenOptions::new().append(true).open(&source).unwrap();
        file.write_all(b"ne\nnext\n").unwrap();
        forwarder.pump();
        assert_eq!(
            fs::read_to_string(&console).unwrap(),
            "[boot]\nhalf a line\nnext\n"
        );

        // A truncated log starts over.
        fs::write(&source, "again\n").unwrap();
        forwarder.pump();
        assert!(fs::read_to_string(&console).unwrap().ends_with("again\n"));
        fs::remove_dir_all(root).ok();
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
