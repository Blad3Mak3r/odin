use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use sysinfo::Signal;

use super::{Instance, InstanceError, process};
use crate::cli::validate_instance_name;
use crate::db::Db;
use crate::paths::{self, Paths};

const STOP_TIMEOUT: Duration = Duration::from_secs(30);

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

/// Starts (creating first, if new) an instance's server process. Returns
/// the `Child` handle so the caller can decide what to do with it: `odin
/// serve` hands it to `web::supervisor` for reaping and console-writer
/// registration; a standalone CLI invocation just drops it (safe — see
/// `process::spawn`'s doc comment), letting it become adoptable by
/// whichever `odin serve` next reconciles.
pub async fn start(
    paths: &Paths,
    db: &Db,
    name: &str,
) -> Result<(Instance, tokio::process::Child)> {
    let mut instance = prepare_start(paths, db, name)?;

    let cmd = process::build_command(&instance, paths)?;
    let child = process::spawn(cmd)
        .await
        .with_context(|| format!("failed to start instance '{name}'"))?;
    let pid = child.id().context("spawned child has no pid")?;
    let pid_started_at = process::start_time_of(pid)?;
    let started_at = Utc::now();

    instance.state.last_started_at = Some(started_at);
    instance.state.pid = Some(pid);
    instance.state.pid_started_at = Some(pid_started_at);
    // Narrower than a full `instance.save`: just the three columns that
    // actually changed, rather than an upsert of every column plus a
    // delete-and-reinsert of every installed mod row.
    crate::db::instances::set_pid(db, name, pid, pid_started_at, started_at)?;

    Ok((instance, child))
}

/// Stops a running instance: SIGINT, wait up to `STOP_TIMEOUT` for a clean
/// exit, SIGKILL as a fallback. Works purely by pid — doesn't require the
/// caller to own the `Child` that was originally spawned, so this is
/// exactly as functional from a standalone CLI invocation as from the web
/// dashboard, and works equally well on an instance "adopted" from a
/// previous `odin serve` boot.
pub async fn stop(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    let mut instance = Instance::load_existing(paths, db, name)?;

    let (Some(pid), Some(pid_started_at)) = (instance.state.pid, instance.state.pid_started_at)
    else {
        bail!(InstanceError::NotRunning(name.to_string()));
    };
    if !process::is_alive(pid, pid_started_at) {
        bail!(InstanceError::NotRunning(name.to_string()));
    }

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

    let stopped_at = Utc::now();
    instance.state.last_stopped_at = Some(stopped_at);
    instance.state.pid = None;
    instance.state.pid_started_at = None;
    crate::db::instances::clear_pid(db, name, stopped_at)?;

    Ok(())
}

/// Stops the instance if it's running, then starts it again. Requires the
/// instance to already exist (unlike `start`, which creates it on demand).
pub async fn restart(
    paths: &Paths,
    db: &Db,
    name: &str,
) -> Result<(Instance, tokio::process::Child)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_env(label: &str) -> (Paths, Db) {
        let dir = std::env::temp_dir().join(format!(
            "odin-lifecycle-e2e-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        (paths, db)
    }

    /// End-to-end smoke test against the *real* `valheim_server.x86_64` on
    /// this host (symlinked in, not copied) — exercises the actual spawn,
    /// liveness, and SIGINT/SIGKILL stop path this whole module was
    /// rewritten around, not just the pure logic in `instance::process`'s
    /// unit tests. Skipped by default (needs a real Valheim dedicated
    /// server install, like the steamcmd-dependent tests in
    /// `valheim_update.rs`): `cargo test -- --ignored lifecycle_e2e`.
    #[tokio::test]
    #[ignore]
    async fn lifecycle_e2e_start_stop_against_a_real_server_binary() {
        // Deliberately bypasses `Paths::resolve`'s system-mode detection
        // (this dev box also has a package-installed `/etc/odin/config.toml`
        // pointing at `/var/lib/odin`, unreadable by this user) — this test
        // only cares about finding *a* real install to symlink in, always
        // under the per-user XDG data dir regardless of system-mode.
        let real_install = directories::ProjectDirs::from("", "", "odin")
            .unwrap()
            .data_dir()
            .join("install")
            .join("valheim");
        assert!(
            real_install.join("valheim_server.x86_64").is_file(),
            "expected a real Valheim install at {}; run `odin install` first",
            real_install.display()
        );

        let (paths, db) = temp_env("smoke");
        std::fs::create_dir_all(paths.shared_install_dir().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real_install, paths.shared_install_dir()).unwrap();

        let (instance, child) = start(&paths, &db, "e2e-smoke").await.unwrap();
        let pid = instance.state.pid.unwrap();
        assert!(process::is_alive(
            pid,
            instance.state.pid_started_at.unwrap()
        ));
        assert!(is_running(&instance).unwrap());
        drop(child); // detach, exactly like the CLI does — must not affect the running process

        // Real console output should show up in the log within a few seconds.
        let console_log = paths::instance_logs_dir(&instance.dir).join("console.log");
        let mut saw_output = false;
        for _ in 0..20 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if std::fs::metadata(&console_log)
                .map(|m| m.len())
                .unwrap_or(0)
                > 0
            {
                saw_output = true;
                break;
            }
        }
        assert!(saw_output, "expected the server to write to console.log");

        stop(&paths, &db, "e2e-smoke").await.unwrap();
        assert!(!process::is_alive(
            pid,
            instance.state.pid_started_at.unwrap()
        ));
        let reloaded = Instance::load_existing(&paths, &db, "e2e-smoke").unwrap();
        assert!(!is_running(&reloaded).unwrap());
        assert_eq!(reloaded.state.pid, None);

        std::fs::remove_dir_all(&paths.data_dir).ok();
    }
}
