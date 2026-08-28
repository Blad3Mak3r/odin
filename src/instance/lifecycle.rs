use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use sysinfo::Signal;

use super::{Instance, InstanceError, process};
use crate::cli::validate_instance_name;
use crate::db::Db;
use crate::paths::{self, Paths};
use crate::supervisor;

const STOP_TIMEOUT: Duration = Duration::from_secs(30);
const SUPERVISOR_START_TIMEOUT: Duration = Duration::from_secs(10);

/// The single canonical liveness check: a live `(pid, pid_started_at)`
/// comparison against the OS process table, never a cached/in-memory
/// value. Correct standalone (no `odin serve` needed) and from the web
/// layer alike.
pub fn is_running(instance: &Instance) -> Result<bool> {
    Ok(match (instance.state.pid, instance.state.pid_started_at) {
        (Some(pid), Some(pid_started_at)) => process::is_alive(pid, pid_started_at),
        _ => false,
    })
}

/// Everything `start` needs to do before actually spawning a process:
/// load-or-create the instance, guard against a double-start, verify the
/// server binary is installed, and prepare the on-disk layout. Split out so
/// `supervisor::server` (the `odin run` supervisor process itself) can call
/// it directly — it no longer goes through `start`'s spawn step, since it
/// *is* what does the spawning now.
pub fn prepare_start(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    let instance = Instance::load_or_create(paths, db, name)?;

    if is_running(&instance)? {
        bail!(InstanceError::AlreadyRunning(name.to_string()));
    }

    let server_binary = paths.shared_install_dir().join("valheim_server.x86_64");
    if !server_binary.is_file() {
        bail!(
            "Valheim dedicated server is not installed (expected {}); run `odin install` first",
            server_binary.display()
        );
    }

    check_port_available(paths, db, &instance)?;
    prepare_instance_layout(paths, &instance)?;

    Ok(instance)
}

/// Starts (creating first, if new) an instance's server process: spawns
/// `odin run --instance <name>` detached (the supervisor process that
/// actually launches and owns Valheim — see `supervisor::server`) and waits
/// for it to become responsive. The supervisor records `(pid,
/// pid_started_at)` in the database itself once it spawns the process, so
/// this just reloads the instance afterwards to pick that up.
pub async fn start(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    prepare_start(paths, db, name)?;

    supervisor::client::spawn_detached(paths, name)
        .await
        .with_context(|| format!("failed to start instance '{name}'"))?;
    supervisor::client::ping_with_retry(paths, name, SUPERVISOR_START_TIMEOUT)
        .await
        .with_context(|| format!("failed to start instance '{name}'"))?;

    Instance::load_existing(paths, db, name)
}

/// Stops a running instance. Prefers asking the live supervisor to do it
/// (`supervisor::client::stop`) — it owns the `Child` directly, so there's
/// no pid-fingerprint race to worry about on its side. Falls back to
/// signalling by pid directly only when no supervisor is reachable: an
/// instance started by a pre-upgrade binary, or one whose supervisor has
/// already crashed. Either way, this function only returns once the
/// process is actually gone (or `STOP_TIMEOUT` has been given a full
/// chance, supervisor-side escalation included).
pub async fn stop(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let instance = Instance::load_existing(paths, db, name)?;

    let (Some(pid), Some(pid_started_at)) = (instance.state.pid, instance.state.pid_started_at)
    else {
        bail!(InstanceError::NotRunning(name.to_string()));
    };
    if !process::is_alive(pid, pid_started_at) {
        bail!(InstanceError::NotRunning(name.to_string()));
    }

    match supervisor::client::stop(paths, name, STOP_TIMEOUT.as_secs()).await {
        Ok(()) => {
            // The supervisor owns the shutdown sequence (SIGINT, then its
            // own SIGKILL escalation after STOP_TIMEOUT) and clears the DB
            // pid itself once the process is gone. Give it a bit more than
            // its own timeout budget so that escalation has time to land.
            if !process::wait_until_gone(
                pid,
                pid_started_at,
                STOP_TIMEOUT + Duration::from_secs(10),
            )
            .await
            {
                bail!(
                    "instance '{name}' did not stop even after the supervisor's own shutdown timeout"
                );
            }
            Ok(())
        }
        Err(_) => stop_via_pid_signal(db, name, pid, pid_started_at).await,
    }
}

/// Direct pid-based stop: SIGINT, wait up to `STOP_TIMEOUT`, SIGKILL as a
/// fallback. The only path taken when no supervisor is reachable for this
/// instance — see `stop`'s doc comment.
async fn stop_via_pid_signal(db: &Db, name: &str, pid: u32, pid_started_at: i64) -> Result<()> {
    process::send_signal(pid, pid_started_at, Signal::Interrupt)?;
    if !process::wait_until_gone(pid, pid_started_at, STOP_TIMEOUT).await {
        tracing::warn!(
            instance = name,
            "graceful shutdown did not complete within {:?}; sending SIGKILL (possible data loss)",
            STOP_TIMEOUT
        );
        process::send_signal(pid, pid_started_at, Signal::Kill)?;
        process::wait_until_gone(pid, pid_started_at, Duration::from_secs(5)).await;
    }

    crate::db::instances::clear_pid(db, name, Utc::now())?;

    Ok(())
}

/// Stops the instance if it's running, then starts it again. Requires the
/// instance to already exist (unlike `start`, which creates it on demand).
pub async fn restart(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    let instance = Instance::load_existing(paths, db, name)?;
    if is_running(&instance)? {
        stop(paths, db, name).await?;
    }
    start(paths, db, name).await
}

/// Renames an instance on disk and in its state, provided it isn't running
/// and no instance already exists under `new_name`. The world name (and thus
/// its save files) is left untouched — only the instance's own identity moves.
pub fn rename(paths: &Paths, db: &Db, old_name: &str, new_name: &str) -> Result<Instance> {
    validate_instance_name(new_name).map_err(InstanceError::InvalidName)?;

    if old_name == new_name {
        bail!("new name is the same as the current name");
    }

    let mut instance = Instance::load_existing(paths, db, old_name)?;

    if is_running(&instance)? {
        bail!(
            "instance '{old_name}' is currently running; run `odin stop {old_name}` before renaming it"
        );
    }

    if Instance::load(paths, db, new_name)?.is_some() {
        bail!(InstanceError::AlreadyExists(new_name.to_string()));
    }

    let new_dir = paths.instance_dir(new_name);
    std::fs::rename(&instance.dir, &new_dir).with_context(|| {
        format!(
            "failed to move instance directory {} to {}",
            instance.dir.display(),
            new_dir.display()
        )
    })?;

    instance.dir = new_dir;
    instance.state.name = new_name.to_string();
    // The instance row is keyed by name, so saving under the new name inserts
    // a fresh row rather than updating the old one — the old row (and its
    // installed_mods) must be deleted explicitly afterwards.
    instance.save(db)?;
    crate::db::instances::delete(db, old_name)?;

    Ok(instance)
}

/// Removes an instance's on-disk directory — optionally preserving its
/// `backups` subdirectory — and its row from the database. Shared by
/// `commands::delete::run` and `web::routes::instances::delete_instance`,
/// which differ only in how they gate/confirm the call, not in what it does.
pub fn delete(db: &Db, instance: &Instance, keep_backups: bool) -> Result<()> {
    if keep_backups {
        let backups_dir = paths::instance_backups_dir(&instance.dir);
        for entry in std::fs::read_dir(&instance.dir)
            .with_context(|| format!("failed to read instance dir {}", instance.dir.display()))?
        {
            let entry = entry?;
            if entry.path() == backups_dir {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            }
            .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    } else {
        std::fs::remove_dir_all(&instance.dir)
            .with_context(|| format!("failed to remove instance dir {}", instance.dir.display()))?;
    }

    crate::db::instances::delete(db, &instance.state.name)?;
    Ok(())
}

fn check_port_available(paths: &Paths, db: &Db, instance: &Instance) -> Result<()> {
    for other in super::list_all(paths, db)? {
        if other.state.name == instance.state.name {
            continue;
        }
        if other.state.port == instance.state.port && is_running(&other)? {
            bail!(
                "port {} is already in use by running instance '{}'",
                instance.state.port,
                other.state.name
            );
        }
    }
    Ok(())
}

fn prepare_instance_layout(paths: &Paths, instance: &Instance) -> Result<()> {
    std::fs::create_dir_all(&instance.dir)
        .with_context(|| format!("failed to create instance dir {}", instance.dir.display()))?;
    std::fs::create_dir_all(paths::instance_saves_dir(&instance.dir))?;
    std::fs::create_dir_all(paths::instance_logs_dir(&instance.dir))?;

    let symlink = paths::instance_server_symlink(&instance.dir);
    if !symlink.exists() {
        std::os::unix::fs::symlink(paths.shared_install_dir(), &symlink).with_context(|| {
            format!(
                "failed to symlink {} -> {}",
                symlink.display(),
                paths.shared_install_dir().display()
            )
        })?;
    }

    Ok(())
}

// The real-Valheim-binary end-to-end test that used to live here moved to
// `supervisor::server`'s test module: `start`'s spawn step now re-execs
// odin's own binary (`std::env::current_exe()`) as `odin run`, which only
// resolves to the real `odin` binary outside a `cargo test` harness — the
// equivalent coverage now drives `supervisor::server::run_instance`
// directly (as `odin run` itself would) instead of going through `start`.
