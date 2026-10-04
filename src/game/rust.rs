//! Rust Dedicated Server's Linux launch contract.

pub mod access_lists;

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
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
    start_unlocked(paths, db, &instance).await
}

async fn start_unlocked(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
) -> Result<RustInstance> {
    if is_running(instance) {
        bail!("instance '{}' is already running", instance.name());
    }
    crate::game::ports::ensure_available(
        db,
        crate::game::GameId::Rust,
        instance.name(),
        [instance.config.port, instance.config.query_port],
    )?;

    let command = build_command(paths, instance)?;
    let child = process::spawn(command)
        .await
        .context("failed to start RustDedicated")?;
    let pid = child.id().context("spawned RustDedicated has no pid")?;
    let pid_started_at = process::start_time_of(pid)?;
    // Dropping Tokio's Child leaves the dedicated server running. Its PID
    // fingerprint is persisted and is the authority for later stop/restart.
    drop(child);
    crate::db::game_instances::set_rust_pid(
        db,
        instance.name(),
        pid,
        pid_started_at,
        chrono::Utc::now(),
    )
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
    let mut command = Command::new(binary);
    command
        .current_dir(&install_dir)
        .arg("-batchmode")
        .arg("-nographics")
        .arg("+server.port")
        .arg(config.port.to_string())
        .arg("+server.queryport")
        .arg(config.query_port.to_string())
        .arg("+server.identity")
        .arg(&instance.identity.id)
        .arg("+server.hostname")
        .arg(&config.hostname)
        .arg("+server.level")
        .arg(&config.level)
        .arg("+server.seed")
        .arg(config.seed.to_string())
        .arg("+server.worldsize")
        .arg(config.world_size.to_string())
        .arg("+server.maxplayers")
        .arg(config.max_players.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0);

    Ok(command)
}

pub async fn stop(paths: &Paths, db: &crate::db::Db, instance: &RustInstance) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    stop_unlocked(db, &instance).await
}

async fn stop_unlocked(db: &crate::db::Db, instance: &RustInstance) -> Result<()> {
    let (Some(pid), Some(started_at)) = (instance.pid, instance.pid_started_at) else {
        bail!("instance '{}' is not running", instance.name());
    };
    if !process::is_alive(pid, started_at) {
        bail!("instance '{}' is not running", instance.name());
    }

    process::send_signal(pid, started_at, Signal::Interrupt)?;
    if !process::wait_until_gone(pid, started_at, STOP_TIMEOUT).await {
        process::send_signal(pid, started_at, Signal::Kill)?;
        if !process::wait_until_gone(pid, started_at, Duration::from_secs(5)).await {
            bail!("instance '{}' did not stop", instance.name());
        }
    }
    crate::db::game_instances::clear_rust_pid(db, instance.name(), chrono::Utc::now())
}

pub async fn restart(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
) -> Result<RustInstance> {
    let _lock = LifecycleLock::acquire(paths, crate::game::GameId::Rust, instance.name())?;
    let instance = refresh(db, instance)?;
    if is_running(&instance) {
        stop_unlocked(db, &instance).await?;
    }
    let refreshed = crate::db::game_instances::load_rust(db, instance.name())?
        .context("Rust instance disappeared while restarting")?;
    start_unlocked(paths, db, &refreshed).await
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
    let instance_dir = paths.game_instance_dir(crate::game::GameId::Rust, instance.name());
    let source = backup_source(paths, &instance);
    if source.exists() {
        std::fs::remove_dir_all(&source).context("failed to delete Rust world data")?;
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
    // Rust itself stores an identity under its shared install tree.  Using the
    // immutable Odin id prevents collisions even when games share a name.
    paths
        .game_install_dir(crate::game::GameId::Rust)
        .join("server")
        .join(&instance.identity.id)
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

pub fn default_config(name: &str, port: u16) -> RustInstanceConfig {
    RustInstanceConfig {
        port,
        query_port: port + 1,
        hostname: name.to_string(),
        level: "Procedural Map".to_string(),
        seed: rand::random(),
        world_size: 3000,
        max_players: 50,
        auto_restart: false,
    }
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

        let started = start(&paths, &db, &instance).await.unwrap();
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
