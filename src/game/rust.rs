//! Rust Dedicated Server's Linux launch contract.

pub mod access_lists;
pub mod rcon;

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::db::game_instances::{RustInstance, RustInstanceConfig};
use crate::instance::lifecycle::LifecycleLock;
use crate::paths::Paths;

pub const DEDICATED_SERVER_APP_ID: &str = "258550";
#[derive(Debug, Clone)]
pub struct RustFileConfig {
    pub hostname: String,
    pub level: String,
    pub seed: u32,
    pub world_size: u32,
    pub max_players: u16,
}

/// Everything `RustDedicated` needs on its command line. Launch arguments only:
/// game settings stay in the native `server.cfg`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustLaunch {
    pub name: String,
    pub identity: String,
    pub port: u16,
    pub query_port: u16,
    pub rcon_port: u16,
    pub rcon_password: String,
}

impl From<&RustInstance> for RustLaunch {
    fn from(instance: &RustInstance) -> Self {
        Self {
            name: instance.name().to_string(),
            identity: instance.identity.id.clone(),
            port: instance.config.port,
            query_port: instance.config.query_port,
            rcon_port: instance.config.rcon_port,
            rcon_password: instance.config.rcon_password.clone(),
        }
    }
}

/// The physical Rust identity directory owned by one Odin instance.
pub fn identity_dir(paths: &Paths, instance: &RustInstance) -> std::path::PathBuf {
    paths
        .game_instance_dir(crate::game::GameId::Rust, instance.name())
        .join("server")
}

fn install_identity_link(paths: &Paths, instance: &RustInstance) -> std::path::PathBuf {
    paths
        .game_install_dir(crate::game::GameId::Rust)
        .join("server")
        .join(&instance.identity.id)
}

/// Makes Rust's conventional `install/server/<identity>` location point at
/// the instance-owned data directory. Existing identities are moved once,
/// never merged, so upgrades cannot silently lose a world or configuration.
pub fn ensure_layout(paths: &Paths, instance: &RustInstance) -> Result<()> {
    let instance_dir = paths.game_instance_dir(crate::game::GameId::Rust, instance.name());
    fs::create_dir_all(&instance_dir)?;
    let local = identity_dir(paths, instance);
    let bridge = install_identity_link(paths, instance);
    if let Ok(metadata) = bridge.symlink_metadata() {
        if metadata.file_type().is_symlink() {
            match fs::canonicalize(&bridge) {
                Ok(target)
                    if target == fs::canonicalize(&local).unwrap_or_else(|_| local.clone()) => {}
                Ok(_) => {
                    bail!(
                        "Rust identity bridge {} belongs to another instance",
                        bridge.display()
                    );
                }
                Err(error) if error.kind() == ErrorKind::NotFound && local.exists() => {
                    fs::remove_file(&bridge).with_context(|| {
                        format!(
                            "failed to replace stale Rust identity bridge {}",
                            bridge.display()
                        )
                    })?;
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to resolve {}", bridge.display()));
                }
            }
        } else if !local.exists() {
            fs::create_dir_all(local.parent().context("Rust identity has no parent")?)?;
            fs::rename(&bridge, &local)
                .with_context(|| format!("failed to migrate {}", bridge.display()))?;
        } else {
            bail!(
                "Rust identity exists both at {} and {}",
                bridge.display(),
                local.display()
            );
        }
    }
    fs::create_dir_all(&local)?;
    if bridge.symlink_metadata().is_err() {
        fs::create_dir_all(bridge.parent().context("Rust bridge has no parent")?)?;
        std::os::unix::fs::symlink(&local, &bridge)
            .with_context(|| format!("failed to create {}", bridge.display()))?;
    }
    Ok(())
}

/// Whether the server's container is running.
pub fn is_running(paths: &Paths, db: &crate::db::Db, instance: &RustInstance) -> Result<bool> {
    crate::podman::rust::is_running(paths, db, instance)
}

/// The single definition of Rust Dedicated's launch contract, used both for
/// the host's direct process and by the Podman runtime with container paths.
pub fn build_command_at(
    install_dir: &Path,
    console_log: &Path,
    launch: &RustLaunch,
) -> Result<Command> {
    let binary = install_dir.join("RustDedicated");
    if !binary.is_file() {
        bail!(
            "Rust Dedicated Server is not installed (expected {}); install Rust first",
            binary.display()
        );
    }
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(console_log)
        .with_context(|| format!("failed to open {}", console_log.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to duplicate Rust log handle")?;

    let library_path = rust_library_path(install_dir)?;
    let mut command = Command::new(binary);
    command
        .current_dir(install_dir)
        .env("LD_LIBRARY_PATH", library_path)
        .arg("-batchmode")
        .arg("-nographics")
        .arg("+server.port")
        .arg(launch.port.to_string())
        .arg("+server.queryport")
        .arg(launch.query_port.to_string())
        .arg("+rcon.port")
        .arg(launch.rcon_port.to_string())
        .arg("+rcon.password")
        .arg(&launch.rcon_password)
        .arg("+rcon.web")
        .arg("1")
        .arg("+server.identity")
        .arg(&launch.identity)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0);

    Ok(command)
}

/// Container-side counterpart of [`ensure_layout`]: exposes the instance's
/// identity directory where `RustDedicated` looks for it and refreshes the
/// Steamworks client link. Both live in the instance's private runtime volume,
/// so the link is repaired before every launch, including after an update.
pub fn prepare_runtime(
    install_dir: &Path,
    instance_dir: &Path,
    steamcmd_dir: &Path,
    launch: &RustLaunch,
) -> Result<()> {
    prepare_runtime_at(
        install_dir,
        instance_dir,
        steamcmd_dir,
        &crate::steamcmd::steam_home_dir()?,
        launch,
    )
}

fn prepare_runtime_at(
    install_dir: &Path,
    instance_dir: &Path,
    steamcmd_dir: &Path,
    steam_home: &Path,
    launch: &RustLaunch,
) -> Result<()> {
    let identity = instance_dir.join("server");
    fs::create_dir_all(&identity)
        .with_context(|| format!("failed to create {}", identity.display()))?;
    let bridge = install_dir.join("server").join(&launch.identity);
    fs::create_dir_all(bridge.parent().context("Rust bridge has no parent")?)?;
    match bridge.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(&bridge)? == identity {
                return link_steam_client_at(steamcmd_dir, steam_home);
            }
            fs::remove_file(&bridge)?;
        }
        Ok(_) => bail!(
            "{} exists and is not a symlink; refusing to replace it",
            bridge.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect {}", bridge.display()));
        }
    }
    std::os::unix::fs::symlink(&identity, &bridge)
        .with_context(|| format!("failed to create {}", bridge.display()))?;
    link_steam_client_at(steamcmd_dir, steam_home)
}

fn link_steam_client_at(steamcmd_dir: &Path, steam_home: &Path) -> Result<()> {
    crate::steamcmd::SteamCmd::new(steamcmd_dir.to_path_buf())
        .ensure_sdk64_client_at(steam_home)
        .context("failed to prepare Rust's Steamworks runtime")?;
    Ok(())
}

fn rust_library_path(install_dir: &Path) -> Result<std::ffi::OsString> {
    let mut paths = vec![
        install_dir.to_path_buf(),
        install_dir.join("RustDedicated_Data/Plugins/x86_64"),
    ];
    if let Some(inherited) = std::env::var_os("LD_LIBRARY_PATH") {
        paths.extend(std::env::split_paths(&inherited));
    }
    std::env::join_paths(paths).context("Rust library search paths contain an invalid ':'")
}

pub fn delete(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
    keep_backups: bool,
) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    if is_running(paths, db, &instance)? {
        anyhow::bail!(crate::instance::InstanceError::AlreadyRunning(
            instance.name().to_string()
        ));
    }
    ensure_layout(paths, &instance)?;
    let instance_dir = paths.game_instance_dir(crate::game::GameId::Rust, instance.name());
    let bridge = install_identity_link(paths, &instance);
    if bridge.symlink_metadata().is_ok() {
        std::fs::remove_file(&bridge)?;
    }
    crate::instance::lifecycle::delete_instance_dir(&instance_dir, keep_backups)?;
    crate::db::game_instances::delete_rust(db, instance.name())
}

/// Deletes Rust's persisted world state without affecting server
/// configuration, blueprints, or Rust+ pairing data. Rust creates a new world
/// from the configured map settings when no `.sav` file remains on its next
/// start.
pub fn wipe_map(paths: &Paths, db: &crate::db::Db, instance: &RustInstance) -> Result<usize> {
    wipe(paths, db, instance, false)
}

/// Deletes Rust's persisted world state and learned blueprints while
/// preserving server configuration and Rust+ pairing data.
pub fn full_wipe(paths: &Paths, db: &crate::db::Db, instance: &RustInstance) -> Result<usize> {
    wipe(paths, db, instance, true)
}

fn wipe(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
    wipe_blueprints: bool,
) -> Result<usize> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    if is_running(paths, db, &instance)? {
        anyhow::bail!(crate::instance::InstanceError::AlreadyRunning(
            instance.name().to_string()
        ));
    }
    ensure_layout(paths, &instance)?;

    let source = backup_source(paths, &instance);
    let entries = match fs::read_dir(&source) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to read Rust world data at {}", source.display())
            });
        }
    };

    let mut wiped = 0;
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read {}", source.display()))?;
        let file_type = entry.file_type().with_context(|| {
            format!(
                "failed to inspect Rust world data at {}",
                entry.path().display()
            )
        })?;
        if !(file_type.is_file() || file_type.is_symlink())
            || !is_wipe_file(&entry, wipe_blueprints)
        {
            continue;
        }
        fs::remove_file(entry.path()).with_context(|| {
            format!(
                "failed to remove Rust world data at {}",
                entry.path().display()
            )
        })?;
        wiped += 1;
    }
    Ok(wiped)
}

fn is_wipe_file(entry: &fs::DirEntry, wipe_blueprints: bool) -> bool {
    entry.file_name().to_str().is_some_and(|name| {
        name.ends_with(".sav")
            || name.contains(".sav.")
            || (wipe_blueprints
                && (name == "player.blueprints"
                    || name.starts_with("player.blueprints-")
                    || name.starts_with("player.blueprints.")))
    })
}

pub fn backup_source(paths: &Paths, instance: &RustInstance) -> std::path::PathBuf {
    identity_dir(paths, instance)
}

fn refresh(db: &crate::db::Db, instance: &RustInstance) -> Result<RustInstance> {
    let current = crate::db::game_instances::load_rust(db, instance.name())?
        .context("Rust instance no longer exists")?;
    anyhow::ensure!(
        current.identity.id == instance.identity.id,
        "Rust instance was replaced"
    );
    Ok(current)
}

pub fn create_backup(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
) -> Result<crate::backup::BackupEntry> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    if is_running(paths, db, &instance)? {
        bail!(
            "stop Rust instance '{}' before creating a backup",
            instance.name()
        );
    }
    ensure_layout(paths, &instance)?;
    create_backup_unlocked(paths, db, &instance)
}

fn create_backup_unlocked(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
) -> Result<crate::backup::BackupEntry> {
    if is_running(paths, db, instance)? {
        bail!(
            "stop Rust instance '{}' before creating a backup",
            instance.name()
        );
    }
    crate::backup::create_at(
        db,
        crate::game::GameId::Rust,
        instance.name(),
        &paths.game_instance_dir(crate::game::GameId::Rust, instance.name()),
        &backup_source(paths, instance),
    )
}

pub fn list_backups(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
) -> Result<Vec<crate::backup::BackupEntry>> {
    // Import pre-registry Rust archives without overwriting remote metadata.
    for entry in crate::backup::list_from_disk(
        &paths.game_instance_dir(crate::game::GameId::Rust, instance.name()),
    )? {
        if crate::db::backups::get_for_game(
            db,
            crate::game::GameId::Rust,
            instance.name(),
            &entry.id,
        )?
        .is_none()
        {
            crate::db::backups::insert_for_game(
                db,
                crate::game::GameId::Rust,
                instance.name(),
                &entry,
            )?;
        }
    }
    crate::db::backups::list_for_game(db, crate::game::GameId::Rust, instance.name())
}

pub fn restore_backup(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
    backup_id: &str,
) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    crate::backup::validate_backup_id(backup_id)?;
    if is_running(paths, db, &instance)? {
        bail!(
            "stop Rust instance '{}' before restoring a backup",
            instance.name()
        );
    }
    ensure_layout(paths, &instance)?;
    list_backups(paths, db, &instance)?;
    crate::backup::restore_at(
        db,
        crate::game::GameId::Rust,
        instance.name(),
        &paths.game_instance_dir(crate::game::GameId::Rust, instance.name()),
        &backup_source(paths, &instance),
        backup_id,
    )
}

pub fn default_config(port: u16) -> RustInstanceConfig {
    RustInstanceConfig {
        port,
        query_port: port + 1,
        rcon_port: port + 2,
        rcon_password: generate_rcon_password(),
        auto_restart: false,
    }
}

pub fn default_file_config(name: &str) -> RustFileConfig {
    RustFileConfig {
        hostname: name.to_string(),
        level: "Procedural Map".to_string(),
        seed: rand::random(),
        world_size: 3000,
        max_players: 50,
    }
}

fn generate_rcon_password() -> String {
    use rand::RngExt;
    use rand::distr::Alphanumeric;

    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::db::game_instances;

    fn temp_context(label: &str) -> (Paths, Db, RustInstance) {
        let dir = std::env::temp_dir().join(format!(
            "odin-rust-driver-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_rust(&paths, &db, "rusty").unwrap();
        (paths, db, instance)
    }

    #[test]
    fn delete_removes_world_data_and_preserves_requested_backups() {
        let (paths, db, instance) = temp_context("delete-world");
        let source = backup_source(&paths, &instance);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("world.sav"), "world").unwrap();
        let backup = create_backup(&paths, &db, &instance).unwrap();
        delete(&paths, &db, &instance, true).unwrap();
        assert!(!source.exists());
        assert!(
            paths
                .game_instance_dir(crate::game::GameId::Rust, instance.name())
                .join("backups")
                .join(format!("{}.zip", backup.id))
                .is_file()
        );
        assert!(
            game_instances::load_rust(&db, instance.name())
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn wipe_map_removes_only_world_saves() {
        let (paths, db, instance) = temp_context("wipe-map");
        let source = backup_source(&paths, &instance);
        assert_eq!(wipe_map(&paths, &db, &instance).unwrap(), 0);
        std::fs::create_dir_all(source.join("cfg")).unwrap();
        for file in ["world.sav", "world.sav.old", "world.sav.1"] {
            std::fs::write(source.join(file), "world").unwrap();
        }
        for file in [
            "world.map",
            "player.blueprints",
            "companion.id",
            "cfg/users.cfg",
        ] {
            std::fs::write(source.join(file), "preserve").unwrap();
        }

        assert_eq!(wipe_map(&paths, &db, &instance).unwrap(), 3);
        for file in ["world.sav", "world.sav.old", "world.sav.1"] {
            assert!(!source.join(file).exists());
        }
        for file in [
            "world.map",
            "player.blueprints",
            "companion.id",
            "cfg/users.cfg",
        ] {
            assert!(source.join(file).is_file());
        }
        assert_eq!(wipe_map(&paths, &db, &instance).unwrap(), 0);

        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn wipe_map_refuses_a_running_instance() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let (paths, db, instance) = temp_context("wipe-map-running");
        let source = backup_source(&paths, &instance);
        std::fs::create_dir_all(&source).unwrap();
        let save = source.join("world.sav");
        std::fs::write(&save, "world").unwrap();
        let socket = paths.data_dir.join("podman.sock");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        crate::podman::test_support::write_podman_config(&paths, &socket);
        let podman = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut String::new())
                .unwrap();
            let body = r#"{"State":{"Running":true,"Status":"running"}}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });

        assert!(wipe_map(&paths, &db, &instance).is_err());
        assert!(save.is_file());

        podman.join().unwrap();
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn full_wipe_removes_blueprints_and_their_journals() {
        let (paths, db, instance) = temp_context("full-wipe");
        let source = backup_source(&paths, &instance);
        std::fs::create_dir_all(&source).unwrap();
        for file in [
            "world.sav",
            "player.blueprints",
            "player.blueprints-wal",
            "player.blueprints-shm",
            "player.blueprints.old",
        ] {
            std::fs::write(source.join(file), "wipe").unwrap();
        }
        std::fs::write(source.join("companion.id"), "preserve").unwrap();

        assert_eq!(full_wipe(&paths, &db, &instance).unwrap(), 5);
        for file in [
            "world.sav",
            "player.blueprints",
            "player.blueprints-wal",
            "player.blueprints-shm",
            "player.blueprints.old",
        ] {
            assert!(!source.join(file).exists());
        }
        assert!(source.join("companion.id").is_file());

        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn backups_cannot_overlap_lifecycle_changes() {
        let (paths, db, instance) = temp_context("backup-lock");
        let _lock =
            LifecycleLock::acquire(&paths, crate::game::GameId::Rust, instance.name()).unwrap();
        assert!(create_backup(&paths, &db, &instance).is_err());
        assert!(restore_backup(&paths, &db, &instance, "backup").is_err());
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn invalid_restore_preserves_current_world() {
        let (paths, db, instance) = temp_context("invalid-restore");
        let source = backup_source(&paths, &instance);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("world.sav"), "current").unwrap();
        let backup = create_backup(&paths, &db, &instance).unwrap();
        let archive = paths
            .game_instance_dir(crate::game::GameId::Rust, instance.name())
            .join("backups")
            .join(format!("{}.zip", backup.id));
        std::fs::write(archive, "not a zip").unwrap();
        assert!(restore_backup(&paths, &db, &instance, &backup.id).is_err());
        assert_eq!(
            std::fs::read_to_string(source.join("world.sav")).unwrap(),
            "current"
        );
        assert!(restore_backup(&paths, &db, &instance, "../escape").is_err());
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn restore_uses_the_selected_backup_even_when_creating_a_safety_snapshot() {
        let (paths, db, instance) = temp_context("backup");
        let source = backup_source(&paths, &instance);
        std::fs::create_dir_all(&source).unwrap();
        let save = source.join("world.sav");
        std::fs::write(&save, "before").unwrap();

        let backup = create_backup(&paths, &db, &instance).unwrap();
        std::fs::write(&save, "after").unwrap();
        restore_backup(&paths, &db, &instance, &backup.id).unwrap();

        assert_eq!(std::fs::read_to_string(save).unwrap(), "before");
        assert_eq!(list_backups(&paths, &db, &instance).unwrap().len(), 2);

        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    fn launch(identity: &str) -> RustLaunch {
        RustLaunch {
            name: "rusty".into(),
            identity: identity.into(),
            port: 28015,
            query_port: 28016,
            rcon_port: 28017,
            rcon_password: "secret".into(),
        }
    }

    fn container_layout(
        label: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("odin-rust-{label}-{}", uuid::Uuid::new_v4()));
        let install = root.join("runtime/rust");
        let instance = root.join("instance");
        let steamcmd = root.join("runtime/steamcmd");
        std::fs::create_dir_all(steamcmd.join("linux64")).unwrap();
        std::fs::write(steamcmd.join("linux64/steamclient.so"), "client").unwrap();
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&instance).unwrap();
        (install, instance, steamcmd)
    }

    #[test]
    fn container_command_is_the_complete_rust_launch_contract() {
        let (install, instance, _steamcmd) = container_layout("command");
        std::fs::write(install.join("RustDedicated"), "").unwrap();
        let log = instance.join("console.log");

        let command = build_command_at(&install, &log, &launch("0e0f")).unwrap();

        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "-batchmode",
                "-nographics",
                "+server.port",
                "28015",
                "+server.queryport",
                "28016",
                "+rcon.port",
                "28017",
                "+rcon.password",
                "secret",
                "+rcon.web",
                "1",
                "+server.identity",
                "0e0f",
            ]
        );
        assert_eq!(command.as_std().get_current_dir(), Some(install.as_path()));
        assert_eq!(
            command.as_std().get_program(),
            install.join("RustDedicated")
        );
        let library_path = command
            .as_std()
            .get_envs()
            .find_map(|(name, value)| (name == "LD_LIBRARY_PATH").then_some(value.unwrap()))
            .unwrap();
        let first_two: Vec<_> = std::env::split_paths(library_path).take(2).collect();
        assert_eq!(
            first_two,
            [
                install.clone(),
                install.join("RustDedicated_Data/Plugins/x86_64")
            ]
        );
        std::fs::remove_dir_all(install.parent().unwrap().parent().unwrap()).ok();
    }

    #[test]
    fn prepare_runtime_exposes_the_identity_and_links_the_steam_client() {
        let (install, instance, steamcmd) = container_layout("prepare");
        let home = install.parent().unwrap().to_path_buf();

        prepare_runtime_at(&install, &instance, &steamcmd, &home, &launch("0e0f")).unwrap();
        prepare_runtime_at(&install, &instance, &steamcmd, &home, &launch("0e0f")).unwrap();

        assert_eq!(
            std::fs::read_link(install.join("server/0e0f")).unwrap(),
            instance.join("server")
        );
        assert!(instance.join("server").is_dir());
        assert_eq!(
            std::fs::read_to_string(home.join(".steam/sdk64/steamclient.so")).unwrap(),
            "client"
        );
        std::fs::remove_dir_all(home.parent().unwrap()).ok();
    }

    #[test]
    fn prepare_runtime_repoints_a_stale_bridge_but_never_replaces_real_data() {
        let (install, instance, steamcmd) = container_layout("stale");
        let home = install.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(install.join("server")).unwrap();
        std::os::unix::fs::symlink("/elsewhere", install.join("server/0e0f")).unwrap();

        prepare_runtime_at(&install, &instance, &steamcmd, &home, &launch("0e0f")).unwrap();
        assert_eq!(
            std::fs::read_link(install.join("server/0e0f")).unwrap(),
            instance.join("server")
        );

        std::fs::create_dir_all(install.join("server/real")).unwrap();
        std::fs::write(install.join("server/real/world.sav"), "world").unwrap();
        assert!(
            prepare_runtime_at(&install, &instance, &steamcmd, &home, &launch("real")).is_err()
        );
        assert!(install.join("server/real/world.sav").is_file());
        std::fs::remove_dir_all(home.parent().unwrap()).ok();
    }
}
