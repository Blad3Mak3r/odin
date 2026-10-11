//! Rust Dedicated Server's Linux launch contract.

pub mod access_lists;
pub mod rcon;

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sysinfo::Signal;
use tokio::process::Command;

use crate::db::game_instances::{RustInstance, RustInstanceConfig};
use crate::instance::{lifecycle::LifecycleLock, process};
use crate::paths::Paths;

pub const DEDICATED_SERVER_APP_ID: &str = "258550";
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct RustFileConfig {
    pub hostname: String,
    pub level: String,
    pub seed: u32,
    pub world_size: u32,
    pub max_players: u16,
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

pub fn is_running(instance: &RustInstance) -> bool {
    matches!(
        (instance.pid, instance.pid_started_at),
        (Some(pid), Some(started_at)) if process::is_alive(pid, started_at)
    )
}

pub async fn start(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
) -> Result<RustInstance> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    start_unlocked(paths, db, instance.name()).await
}

async fn start_unlocked(paths: &Paths, db: &crate::db::Db, name: &str) -> Result<RustInstance> {
    let instance = prepare_start(paths, db, name)?;
    crate::supervisor::client::spawn_detached(paths, &instance.identity)
        .await
        .with_context(|| format!("failed to start Rust instance '{name}'"))?;
    crate::supervisor::client::ping_with_retry(
        paths,
        crate::game::GameId::Rust,
        name,
        Duration::from_secs(10),
    )
    .await
    .with_context(|| format!("failed to start Rust instance '{name}'"))?;
    crate::db::game_instances::load_rust(db, name)?.context("Rust instance disappeared after start")
}

/// Performs the synchronous checks and setup required before the Rust
/// supervisor launches `RustDedicated`. It is also called by `odin run`, so
/// the hidden supervisor command remains safe when invoked independently.
pub fn prepare_start(paths: &Paths, db: &crate::db::Db, name: &str) -> Result<RustInstance> {
    let instance =
        crate::db::game_instances::load_rust(db, name)?.context("Rust instance does not exist")?;
    if is_running(&instance) {
        bail!("instance '{}' is already running", instance.name());
    }
    ensure_layout(paths, &instance)?;
    crate::game::ports::ensure_available(
        db,
        crate::game::GameId::Rust,
        instance.name(),
        [
            instance.config.port,
            instance.config.query_port,
            instance.config.rcon_port,
        ],
    )?;

    crate::steamcmd::SteamCmd::new(paths.steamcmd_dir())
        .ensure_sdk64_client_at(&crate::steamcmd::steam_home_dir()?)
        .context("failed to prepare Rust's Steamworks runtime")?;
    crate::game::config_documents::sync_rust_operational(paths, &instance)?;
    build_command(paths, &instance)?;
    Ok(instance)
}

/// Builds Rust Dedicated's process command using Odin's game-isolated
/// instance layout. Spawning itself is shared with Valheim through
/// [`process::spawn`].
pub fn build_command(paths: &Paths, instance: &RustInstance) -> Result<Command> {
    let install_dir = paths.game_install_dir(crate::game::GameId::Rust);
    let binary = install_dir.join("RustDedicated");
    if !binary.is_file() {
        bail!(
            "Rust Dedicated Server is not installed (expected {}); install Rust first",
            binary.display()
        );
    }
    let instance_dir = paths.game_instance_dir(crate::game::GameId::Rust, instance.name());
    let log_dir = instance_dir.join("logs");
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create {}", log_dir.display()))?;
    let log_path = log_dir.join("console.log");
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to duplicate Rust log handle")?;

    let config = &instance.config;
    let library_path = rust_library_path(&install_dir)?;
    let mut command = Command::new(binary);
    command
        .current_dir(&install_dir)
        .env("LD_LIBRARY_PATH", library_path)
        .arg("-batchmode")
        .arg("-nographics")
        .arg("+server.port")
        .arg(config.port.to_string())
        .arg("+server.queryport")
        .arg(config.query_port.to_string())
        .arg("+rcon.port")
        .arg(config.rcon_port.to_string())
        .arg("+rcon.password")
        .arg(&config.rcon_password)
        .arg("+rcon.web")
        .arg("1")
        .arg("+server.identity")
        .arg(&instance.identity.id)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0);

    Ok(command)
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

pub async fn stop(paths: &Paths, db: &crate::db::Db, instance: &RustInstance) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    stop_unlocked(paths, db, &instance).await
}

async fn stop_unlocked(paths: &Paths, db: &crate::db::Db, instance: &RustInstance) -> Result<()> {
    let (Some(pid), Some(started_at)) = (instance.pid, instance.pid_started_at) else {
        bail!("instance '{}' is not running", instance.name());
    };
    if !process::is_alive(pid, started_at) {
        bail!("instance '{}' is not running", instance.name());
    }

    match crate::supervisor::client::stop(
        paths,
        crate::game::GameId::Rust,
        instance.name(),
        STOP_TIMEOUT.as_secs(),
    )
    .await
    {
        Ok(()) => {
            if !process::wait_until_gone(pid, started_at, STOP_TIMEOUT + Duration::from_secs(10))
                .await
            {
                bail!("Rust instance '{}' did not stop", instance.name());
            }
            Ok(())
        }
        Err(_) => stop_via_pid_signal(db, instance.name(), pid, started_at).await,
    }
}

pub async fn restart(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
) -> Result<RustInstance> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    if is_running(&instance) {
        stop_unlocked(paths, db, &instance).await?;
    }
    ensure_layout(paths, &instance)?;
    start_unlocked(paths, db, instance.name()).await
}

async fn stop_via_pid_signal(
    db: &crate::db::Db,
    name: &str,
    pid: u32,
    started_at: i64,
) -> Result<()> {
    process::send_signal(pid, started_at, Signal::Interrupt)?;
    if !process::wait_until_gone(pid, started_at, STOP_TIMEOUT).await {
        process::send_signal(pid, started_at, Signal::Kill)?;
        if !process::wait_until_gone(pid, started_at, Duration::from_secs(5)).await {
            bail!("Rust instance '{name}' did not stop");
        }
    }
    crate::db::game_instances::clear_rust_pid(db, name, chrono::Utc::now())
}

pub fn delete(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
    keep_backups: bool,
) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    if is_running(&instance) {
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
    if is_running(&instance) {
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
    if is_running(&instance) {
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
    if is_running(instance) {
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
    if is_running(&instance) {
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
    use std::os::unix::fs::PermissionsExt;

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

    fn install_fake_server(paths: &Paths) {
        let install_dir = paths.game_install_dir(crate::game::GameId::Rust);
        std::fs::create_dir_all(&install_dir).unwrap();
        let binary = install_dir.join("RustDedicated");
        std::fs::write(
            &binary,
            "#!/bin/sh\necho fake Rust server started\nexec sleep 1000\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn install_fake_steam_client(paths: &Paths) {
        let client = paths.steamcmd_dir().join("linux64/steamclient.so");
        std::fs::create_dir_all(client.parent().unwrap()).unwrap();
        std::fs::write(client, "fake Steam client").unwrap();
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
        let (paths, db, instance) = temp_context("wipe-map-running");
        let source = backup_source(&paths, &instance);
        std::fs::create_dir_all(&source).unwrap();
        let save = source.join("world.sav");
        std::fs::write(&save, "world").unwrap();
        game_instances::set_rust_pid(
            &db,
            instance.name(),
            std::process::id(),
            crate::instance::process::start_time_of(std::process::id()).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();

        assert!(wipe_map(&paths, &db, &instance).is_err());
        assert!(save.is_file());

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

    #[tokio::test]
    async fn fake_server_start_and_stop_persist_the_process_lifecycle() {
        let (paths, db, instance) = temp_context("lifecycle");
        install_fake_server(&paths);
        install_fake_steam_client(&paths);

        let child = process::spawn(build_command(&paths, &instance).unwrap())
            .await
            .unwrap();
        let pid = child.id().unwrap();
        let started_at = process::start_time_of(pid).unwrap();
        drop(child);
        game_instances::set_rust_pid(&db, instance.name(), pid, started_at, chrono::Utc::now())
            .unwrap();
        let started = game_instances::load_rust(&db, instance.name())
            .unwrap()
            .unwrap();
        assert!(is_running(&started));

        tokio::time::sleep(Duration::from_millis(50)).await;
        let log = paths
            .game_instance_dir(crate::game::GameId::Rust, started.name())
            .join("logs/console.log");
        assert!(
            std::fs::read_to_string(log)
                .unwrap()
                .contains("fake Rust server started")
        );

        stop(&paths, &db, &started).await.unwrap();
        let stopped = game_instances::load_rust(&db, started.name())
            .unwrap()
            .unwrap();
        assert!(!is_running(&stopped));
        assert!(stopped.pid.is_none());

        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn rust_command_prepends_its_native_library_directories() {
        let (paths, _db, instance) = temp_context("library-path");
        install_fake_server(&paths);

        let command = build_command(&paths, &instance).unwrap();
        let configured = command
            .as_std()
            .get_envs()
            .find_map(|(name, value)| (name == "LD_LIBRARY_PATH").then_some(value.unwrap()))
            .unwrap();
        let configured: Vec<_> = std::env::split_paths(configured).collect();
        let install_dir = paths.game_install_dir(crate::game::GameId::Rust);
        let mut expected = vec![
            install_dir.clone(),
            install_dir.join("RustDedicated_Data/Plugins/x86_64"),
        ];
        if let Some(inherited) = std::env::var_os("LD_LIBRARY_PATH") {
            expected.extend(std::env::split_paths(&inherited));
        }
        assert_eq!(configured, expected);

        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn rust_command_enables_web_rcon_with_the_instance_configuration() {
        let (paths, _db, instance) = temp_context("rcon");
        install_fake_server(&paths);

        let command = build_command(&paths, &instance).unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert!(args.windows(2).any(
            |args| args[0] == "+rcon.port" && args[1] == instance.config.rcon_port.to_string()
        ));
        assert!(
            args.windows(2)
                .any(|args| args[0] == "+rcon.password" && args[1] == instance.config.rcon_password)
        );
        assert!(args.windows(2).any(|args| args == ["+rcon.web", "1"]));
        for file_owned in [
            "+server.hostname",
            "+server.level",
            "+server.seed",
            "+server.worldsize",
            "+server.maxplayers",
        ] {
            assert!(!args.iter().any(|argument| argument == file_owned));
        }

        std::fs::remove_dir_all(paths.data_dir).ok();
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
}
