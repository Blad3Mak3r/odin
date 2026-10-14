//! Counter-Strike 2's process-level launch contract.
//!
//! Valve ships a native Linux dedicated server in Steam app 730. Odin passes
//! only what the process needs from it (ports, start map, game mode, player
//! limit, the optional login token and the RCON password); every other
//! setting stays in the game's own `server.cfg`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::db::game_instances::GameInstanceIdentity;
use crate::game::GameId;
use crate::paths::Paths;

/// Relative to the install directory. `cs2.sh` is the wrapper Valve ships to
/// start the server; the binary under `bin/linuxsteamrt64` is not meant to be
/// run directly.
pub const SERVER_BINARY: &str = "game/cs2.sh";
/// Relative to the install directory: the document the server executes when
/// a map loads. The container mounts the instance's own copy over it.
pub const CONFIG_FILE: &str = "game/csgo/cfg/server.cfg";
/// Relative to the instance directory: the instance's own `server.cfg`.
pub const CONFIG_PATH: &str = "cfg/server.cfg";
/// Relative to a shared install: the SteamCMD the shared installer used.
const SHARED_STEAMCMD_DIR: &str = ".odin-steamcmd";

pub const DEFAULT_MAP: &str = "de_dust2";
pub const DEFAULT_MAX_PLAYERS: u8 = 10;
pub const MAX_PLAYERS: u8 = 64;

/// SourceTV listens on its own UDP port, which defaults to the same value for
/// every process. The driver derives it from the game port so that instances
/// spaced by the usual port step never share it.
const TV_PORT_OFFSET: u16 = 5;

pub fn tv_port(port: u16) -> Result<u16> {
    port.checked_add(TV_PORT_OFFSET)
        .context("no available SourceTV port remains for the game port")
}

/// The game modes Odin offers, as the `game_type` / `game_mode` pair the
/// server expects on its command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cs2GameMode {
    Casual,
    Competitive,
    Wingman,
    Deathmatch,
}

impl Cs2GameMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Casual => "casual",
            Self::Competitive => "competitive",
            Self::Wingman => "wingman",
            Self::Deathmatch => "deathmatch",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "casual" => Ok(Self::Casual),
            "competitive" => Ok(Self::Competitive),
            "wingman" => Ok(Self::Wingman),
            "deathmatch" => Ok(Self::Deathmatch),
            other => bail!("unsupported Counter-Strike 2 game mode '{other}'"),
        }
    }

    const fn game_type_and_mode(self) -> (u8, u8) {
        match self {
            Self::Casual => (0, 0),
            Self::Competitive => (0, 1),
            Self::Wingman => (0, 2),
            Self::Deathmatch => (1, 2),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cs2LaunchOptions {
    pub map: String,
    pub game_mode: Cs2GameMode,
    pub max_players: u8,
    /// Game Server Login Token. Without one the server runs but is not
    /// listed publicly. It must reach the process on the command line: the
    /// server logs in to Steam before it reads `server.cfg`.
    #[serde(default)]
    pub gslt: Option<String>,
}

impl Cs2LaunchOptions {
    pub fn defaults() -> Self {
        Self {
            map: DEFAULT_MAP.into(),
            game_mode: Cs2GameMode::Competitive,
            max_players: DEFAULT_MAX_PLAYERS,
            gslt: None,
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
            "Counter-Strike 2 map must contain 1 to 64 ASCII letters, numbers, or underscores"
        );
        anyhow::ensure!(
            (1..=MAX_PLAYERS).contains(&self.max_players),
            "Counter-Strike 2 supports 1 to {MAX_PLAYERS} players"
        );
        if let Some(token) = &self.gslt {
            anyhow::ensure!(
                !token.is_empty()
                    && token.len() <= 64
                    && token.bytes().all(|byte| byte.is_ascii_alphanumeric()),
                "the Counter-Strike 2 login token must contain 1 to 64 ASCII letters or numbers"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cs2Launch {
    pub name: String,
    pub port: u16,
    pub tv_port: u16,
    pub rcon_password: String,
    #[serde(flatten)]
    pub options: Cs2LaunchOptions,
}

impl Cs2Launch {
    pub fn validate(&self) -> Result<()> {
        self.options.validate()?;
        anyhow::ensure!(
            !self.rcon_password.trim().is_empty(),
            "Counter-Strike 2 RCON password cannot be empty"
        );
        Ok(())
    }
}

/// Where the `server.cfg` the installed server ships lives for an instance: its
/// private copy of the game once installed, otherwise the shared installation.
/// `None` when the game ships none.
pub fn template_path(paths: &Paths, identity: &GameInstanceIdentity) -> Option<PathBuf> {
    [
        paths
            .container_runtime_dir(GameId::Cs2, &identity.id)
            .join(GameId::Cs2.as_str()),
        paths.shared_install_dir(GameId::Cs2),
    ]
    .into_iter()
    .map(|install| install.join(CONFIG_FILE))
    .find(|candidate| candidate.is_file())
}

/// Where the Steamworks client library comes from: the instance's own
/// SteamCMD, or the one the shared installer left beside the shared files.
fn steamcmd_dir(install_dir: &Path, steamcmd_dir: &Path) -> PathBuf {
    let shared = install_dir.join(SHARED_STEAMCMD_DIR);
    if steamcmd_dir.join("linux64/steamclient.so").is_file() || !shared.is_dir() {
        steamcmd_dir.to_path_buf()
    } else {
        shared
    }
}

/// Container-side preparation, run before every launch. The server refuses to
/// start without `$HOME/.steam/sdk64/steamclient.so`, so the link is repaired
/// every time. An instance with its own game files also gets its `server.cfg`
/// replaced by a link to the instance's document: the server executes that
/// path whenever a map loads. (A shared instance gets the document mounted
/// over it instead, because the shared files are not ours to change.)
pub fn prepare_runtime(
    install_dir: &Path,
    instance_dir: &Path,
    steamcmd_dir_hint: &Path,
    shared_install: bool,
) -> Result<()> {
    prepare_runtime_at(
        install_dir,
        instance_dir,
        steamcmd_dir_hint,
        &crate::steamcmd::steam_home_dir()?,
        shared_install,
    )
}

fn prepare_runtime_at(
    install_dir: &Path,
    instance_dir: &Path,
    steamcmd_dir_hint: &Path,
    steam_home: &Path,
    shared_install: bool,
) -> Result<()> {
    crate::steamcmd::SteamCmd::new(steamcmd_dir(install_dir, steamcmd_dir_hint))
        .ensure_sdk64_client_at(steam_home)
        .context("failed to prepare Counter-Strike 2's Steamworks runtime")?;
    if !shared_install {
        link_config(install_dir, instance_dir)?;
    }
    Ok(())
}

fn link_config(install_dir: &Path, instance_dir: &Path) -> Result<()> {
    let document = instance_dir.join(CONFIG_PATH);
    let target = install_dir.join(CONFIG_FILE);
    let parent = target
        .parent()
        .context("Counter-Strike 2 configuration path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    match target.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(&target)? == document {
                return Ok(());
            }
            fs::remove_file(&target)?;
        }
        // The file the game ships. An operator's document is never replaced,
        // but a missing one starts from it.
        Ok(metadata) if metadata.is_file() => {
            if !document.exists() {
                let document_parent = document
                    .parent()
                    .context("Counter-Strike 2 document path has no parent")?;
                fs::create_dir_all(document_parent)?;
                fs::copy(&target, &document).with_context(|| {
                    format!(
                        "failed to seed {} from {}",
                        document.display(),
                        target.display()
                    )
                })?;
            }
            fs::remove_file(&target)?;
        }
        Ok(_) => bail!(
            "{} exists and is not a file or a symlink; refusing to replace it",
            target.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect {}", target.display()));
        }
    }
    if !document.exists() {
        let document_parent = document
            .parent()
            .context("Counter-Strike 2 document path has no parent")?;
        fs::create_dir_all(document_parent)?;
        fs::write(&document, "")
            .with_context(|| format!("failed to create {}", document.display()))?;
    }
    std::os::unix::fs::symlink(&document, &target)
        .with_context(|| format!("failed to link {}", target.display()))
}

/// The single definition of the server's launch contract. The server runs
/// from `game/` and keeps its own logs and `server.cfg` under `csgo/`, which
/// the container mounts per instance.
pub fn build_command_at(
    install_dir: &Path,
    console_log: &Path,
    launch: &Cs2Launch,
) -> Result<tokio::process::Command> {
    launch.validate()?;
    let binary = install_dir.join(SERVER_BINARY);
    if !binary.is_file() {
        bail!(
            "Counter-Strike 2 is not installed (expected {})",
            binary.display()
        );
    }
    let cwd = binary
        .parent()
        .context("Counter-Strike 2 server binary has no parent")?;
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(console_log)
        .with_context(|| format!("failed to open {}", console_log.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to duplicate Counter-Strike 2 log handle")?;
    let (game_type, game_mode) = launch.options.game_mode.game_type_and_mode();
    let mut command = tokio::process::Command::new(&binary);
    command
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr))
        .process_group(0)
        .args(["-dedicated", "-console", "-usercon"])
        // Without it the server binds its TCP (RCON) listener to the address
        // the hostname resolves to, which is not the loopback address Odin
        // connects to.
        .args(["-ip", "0.0.0.0"])
        .arg("-port")
        .arg(launch.port.to_string())
        .arg("-maxplayers")
        .arg(launch.options.max_players.to_string())
        .args(["+game_type", &game_type.to_string()])
        .args(["+game_mode", &game_mode.to_string()])
        .args(["+map", &launch.options.map])
        .args(["+rcon_password", &launch.rcon_password])
        .args(["+tv_port", &launch.tv_port.to_string()]);
    if let Some(token) = &launch.options.gslt {
        command.args(["+sv_setsteamaccount", token]);
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> Cs2Launch {
        Cs2Launch {
            name: "dust".into(),
            port: 27015,
            tv_port: 27020,
            rcon_password: "secret".into(),
            options: Cs2LaunchOptions {
                map: "de_inferno".into(),
                game_mode: Cs2GameMode::Wingman,
                max_players: 8,
                gslt: Some("ABCDEF0123456789".into()),
            },
        }
    }

    fn installed(root: &Path) -> std::path::PathBuf {
        let install = root.join("install");
        let binary = install.join(SERVER_BINARY);
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, "").unwrap();
        install
    }

    fn arguments(command: &tokio::process::Command) -> Vec<String> {
        command
            .as_std()
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_command_is_the_complete_server_contract() {
        let root = std::env::temp_dir().join(format!("odin-cs2-command-{}", uuid::Uuid::new_v4()));
        let install = installed(&root);
        let log = root.join("console.log");

        let command = build_command_at(&install, &log, &launch()).unwrap();

        assert_eq!(
            command.as_std().get_program(),
            install.join(SERVER_BINARY).as_os_str()
        );
        assert_eq!(
            command.as_std().get_current_dir(),
            install.join(SERVER_BINARY).parent()
        );
        assert_eq!(
            arguments(&command),
            [
                "-dedicated",
                "-console",
                "-usercon",
                "-ip",
                "0.0.0.0",
                "-port",
                "27015",
                "-maxplayers",
                "8",
                "+game_type",
                "0",
                "+game_mode",
                "2",
                "+map",
                "de_inferno",
                "+rcon_password",
                "secret",
                "+tv_port",
                "27020",
                "+sv_setsteamaccount",
                "ABCDEF0123456789",
            ]
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_login_token_is_omitted_when_there_is_none() {
        let root = std::env::temp_dir().join(format!("odin-cs2-no-token-{}", uuid::Uuid::new_v4()));
        let install = installed(&root);
        let mut launch = launch();
        launch.options.gslt = None;

        let command = build_command_at(&install, &root.join("console.log"), &launch).unwrap();

        assert!(!arguments(&command).contains(&"+sv_setsteamaccount".to_string()));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn each_game_mode_maps_to_its_type_and_mode_pair() {
        let pairs = [
            (Cs2GameMode::Casual, (0, 0)),
            (Cs2GameMode::Competitive, (0, 1)),
            (Cs2GameMode::Wingman, (0, 2)),
            (Cs2GameMode::Deathmatch, (1, 2)),
        ];
        for (mode, pair) in pairs {
            assert_eq!(mode.game_type_and_mode(), pair);
            assert_eq!(Cs2GameMode::parse(mode.as_str()).unwrap(), mode);
        }
        assert!(Cs2GameMode::parse("premier").is_err());
    }

    #[test]
    fn a_missing_install_is_reported_as_not_installed() {
        let root = std::env::temp_dir().join(format!("odin-cs2-missing-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();

        let error = build_command_at(&root, &root.join("console.log"), &launch()).unwrap_err();

        assert!(error.to_string().contains("not installed"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn launch_options_reject_values_the_server_cannot_take() {
        assert!(Cs2LaunchOptions::defaults().validate().is_ok());
        for options in [
            Cs2LaunchOptions {
                map: "de dust".into(),
                ..Cs2LaunchOptions::defaults()
            },
            Cs2LaunchOptions {
                map: String::new(),
                ..Cs2LaunchOptions::defaults()
            },
            Cs2LaunchOptions {
                max_players: 0,
                ..Cs2LaunchOptions::defaults()
            },
            Cs2LaunchOptions {
                max_players: 65,
                ..Cs2LaunchOptions::defaults()
            },
            Cs2LaunchOptions {
                gslt: Some("token with spaces".into()),
                ..Cs2LaunchOptions::defaults()
            },
            Cs2LaunchOptions {
                gslt: Some(String::new()),
                ..Cs2LaunchOptions::defaults()
            },
        ] {
            assert!(options.validate().is_err(), "{options:?}");
        }
    }

    struct Runtime {
        root: PathBuf,
        install: PathBuf,
        instance: PathBuf,
        steamcmd: PathBuf,
        home: PathBuf,
    }

    impl Runtime {
        fn new(label: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("odin-cs2-{label}-{}", uuid::Uuid::new_v4()));
            let runtime = Self {
                install: root.join("install"),
                instance: root.join("instance"),
                steamcmd: root.join("steamcmd"),
                home: root.join("home"),
                root,
            };
            fs::create_dir_all(runtime.install.join(CONFIG_FILE).parent().unwrap()).unwrap();
            fs::create_dir_all(runtime.steamcmd.join("linux64")).unwrap();
            fs::write(runtime.steamcmd.join("linux64/steamclient.so"), "client").unwrap();
            fs::write(runtime.install.join(CONFIG_FILE), "// shipped\n").unwrap();
            runtime
        }

        fn prepare(&self, shared: bool) -> Result<()> {
            prepare_runtime_at(
                &self.install,
                &self.instance,
                &self.steamcmd,
                &self.home,
                shared,
            )
        }

        fn document(&self) -> PathBuf {
            self.instance.join(CONFIG_PATH)
        }
    }

    impl Drop for Runtime {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }

    #[test]
    fn an_isolated_instance_links_its_document_and_starts_it_from_the_shipped_file() {
        let runtime = Runtime::new("link");

        runtime.prepare(false).unwrap();
        runtime.prepare(false).unwrap();

        assert_eq!(
            fs::read_to_string(runtime.document()).unwrap(),
            "// shipped\n"
        );
        let target = runtime.install.join(CONFIG_FILE);
        assert_eq!(fs::read_link(&target).unwrap(), runtime.document());
        assert!(runtime.home.join(".steam/sdk64/steamclient.so").is_file());
    }

    #[test]
    fn an_existing_document_is_never_overwritten() {
        let runtime = Runtime::new("keep");
        fs::create_dir_all(runtime.document().parent().unwrap()).unwrap();
        fs::write(runtime.document(), "hostname \"mine\"\n").unwrap();

        runtime.prepare(false).unwrap();

        assert_eq!(
            fs::read_to_string(runtime.document()).unwrap(),
            "hostname \"mine\"\n"
        );
        // A game update brings the shipped file back; the next launch links
        // the document again without touching it.
        fs::remove_file(runtime.install.join(CONFIG_FILE)).unwrap();
        fs::write(runtime.install.join(CONFIG_FILE), "// shipped again\n").unwrap();
        runtime.prepare(false).unwrap();
        assert_eq!(
            fs::read_to_string(runtime.install.join(CONFIG_FILE)).unwrap(),
            "hostname \"mine\"\n"
        );
    }

    #[test]
    fn a_shared_instance_leaves_the_shared_files_alone() {
        let runtime = Runtime::new("shared");

        runtime.prepare(true).unwrap();

        let target = runtime.install.join(CONFIG_FILE);
        assert!(!target.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(target).unwrap(), "// shipped\n");
        assert!(!runtime.document().exists());
        assert!(runtime.home.join(".steam/sdk64/steamclient.so").is_file());
    }

    #[test]
    fn a_directory_in_place_of_the_document_is_refused() {
        let runtime = Runtime::new("directory");
        let target = runtime.install.join(CONFIG_FILE);
        fs::remove_file(&target).unwrap();
        fs::create_dir(&target).unwrap();

        assert!(runtime.prepare(false).is_err());
    }

    #[test]
    fn the_steam_client_comes_from_the_shared_installer_when_there_is_no_private_one() {
        let runtime = Runtime::new("client");
        let shared = runtime.install.join(SHARED_STEAMCMD_DIR);
        fs::create_dir_all(shared.join("linux64")).unwrap();
        fs::write(shared.join("linux64/steamclient.so"), "shared client").unwrap();
        fs::remove_dir_all(runtime.steamcmd.join("linux64")).unwrap();

        runtime.prepare(true).unwrap();

        assert_eq!(
            fs::read_to_string(runtime.home.join(".steam/sdk64/steamclient.so")).unwrap(),
            "shared client"
        );
    }

    #[test]
    fn a_missing_steam_client_stops_the_launch() {
        let runtime = Runtime::new("no-client");
        fs::remove_dir_all(runtime.steamcmd.join("linux64")).unwrap();

        assert!(runtime.prepare(false).is_err());
    }

    #[test]
    fn the_tv_port_follows_the_game_port() {
        assert_eq!(tv_port(27015).unwrap(), 27020);
        assert!(tv_port(u16::MAX).is_err());
    }
}
