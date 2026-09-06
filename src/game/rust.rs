//! Rust Dedicated Server's Linux launch contract.

use std::fs::OpenOptions;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
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
    start_unlocked(paths, db, instance).await
}

async fn start_unlocked(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
) -> Result<RustInstance> {
    if is_running(instance) {
        bail!("instance '{}' is already running", instance.name());
    }

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
    stop_unlocked(db, instance).await
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
    if is_running(instance) {
        stop_unlocked(db, instance).await?;
    }
    let refreshed = crate::db::game_instances::load_rust(db, instance.name())?
        .context("Rust instance disappeared while restarting")?;
    start_unlocked(paths, db, &refreshed).await
}

pub fn backup_source(paths: &Paths, instance: &RustInstance) -> std::path::PathBuf {
    // Rust itself stores an identity under its shared install tree.  Using the
    // immutable Odin id prevents collisions even when games share a name.
    paths
        .game_install_dir(crate::game::GameId::Rust)
        .join("server")
        .join(&instance.identity.id)
}

pub fn create_backup(paths: &Paths, instance: &RustInstance) -> Result<crate::backup::BackupEntry> {
    if is_running(instance) {
        bail!(
            "stop Rust instance '{}' before creating a backup",
            instance.name()
        );
    }
    let backups_dir = paths
        .game_instance_dir(crate::game::GameId::Rust, instance.name())
        .join("backups");
    std::fs::create_dir_all(&backups_dir)?;
    let id = crate::backup::backup_id_now();
    let path = backups_dir.join(format!("{id}.zip"));
    crate::backup::zip_directory(&backup_source(paths, instance), &path)?;
    Ok(crate::backup::BackupEntry {
        id,
        created_at: Utc::now(),
        size_bytes: std::fs::metadata(path)?.len(),
        storage: crate::backup::BackupStorage::Local,
    })
}

pub fn list_backups(
    paths: &Paths,
    instance: &RustInstance,
) -> Result<Vec<crate::backup::BackupEntry>> {
    crate::backup::list_from_disk(
        &paths.game_instance_dir(crate::game::GameId::Rust, instance.name()),
    )
}

pub fn restore_backup(paths: &Paths, instance: &RustInstance, backup_id: &str) -> Result<()> {
    if is_running(instance) {
        bail!(
            "stop Rust instance '{}' before restoring a backup",
            instance.name()
        );
    }
    let backups_dir = paths
        .game_instance_dir(crate::game::GameId::Rust, instance.name())
        .join("backups");
    let archive = backups_dir.join(format!("{backup_id}.zip"));
    if !archive.is_file() {
        bail!("backup '{backup_id}' not found");
    }
    create_backup(paths, instance).context("failed to snapshot Rust data before restore")?;
    let source = backup_source(paths, instance);
    std::fs::remove_dir_all(&source).ok();
    std::fs::create_dir_all(&source)?;
    crate::mods::extract_zip_to_dir(&archive, &source)
        .with_context(|| format!("failed to restore backup '{backup_id}'"))
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
        let (paths, _db, instance) = temp_context("backup");
        let source = backup_source(&paths, &instance);
        std::fs::create_dir_all(&source).unwrap();
        let save = source.join("world.sav");
        std::fs::write(&save, "before").unwrap();

        let backup = create_backup(&paths, &instance).unwrap();
        std::fs::write(&save, "after").unwrap();
        restore_backup(&paths, &instance, &backup.id).unwrap();

        assert_eq!(std::fs::read_to_string(save).unwrap(), "before");
        assert_eq!(list_backups(&paths, &instance).unwrap().len(), 2);

        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
