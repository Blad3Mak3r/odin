use std::fmt::Write;
use std::fs::{File, OpenOptions, TryLockError};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use super::{Instance, InstanceError};
use crate::cli::validate_instance_name;
use crate::db::Db;
use crate::game::GameId;
use crate::paths::{self, Paths};

/// Cross-process, per-instance exclusion for lifecycle transitions. The
/// empty lock file is intentionally persistent; the OS releases the lock
/// automatically if the owning request/process exits unexpectedly.
#[derive(Debug)]
pub(crate) struct LifecycleLock {
    _file: File,
}

impl LifecycleLock {
    pub(crate) fn acquire(paths: &Paths, game: GameId, name: &str) -> Result<Self> {
        validate_instance_name(name).map_err(InstanceError::InvalidName)?;
        let lock_dir = paths.runtime_dir().join("lifecycle-locks");
        std::fs::create_dir_all(&lock_dir)
            .with_context(|| format!("failed to create {}", lock_dir.display()))?;
        let path = lock_dir.join(lock_filename(paths, game, name));
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open lifecycle lock {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => {
                bail!(InstanceError::TransitionInProgress(name.to_string()))
            }
            Err(TryLockError::Error(error)) => Err(error)
                .with_context(|| format!("failed to lock lifecycle file {}", path.display())),
        }
    }
}

/// Derives a fixed-format filename so even a future caller that bypasses the
/// instance-name validator cannot turn a lifecycle lock into a path escape.
/// The data root and game id keep independent Odin installations and
/// same-named instances in separate lock domains.
fn lock_filename(paths: &Paths, game: GameId, name: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(paths.data_dir.to_string_lossy().as_bytes());
    digest.update([0]);
    digest.update(game.as_str());
    digest.update([0]);
    digest.update(name.as_bytes());
    let mut filename = String::with_capacity(64 + ".lifecycle.lock".len());
    for byte in digest.finalize() {
        write!(filename, "{byte:02x}").expect("writing a lock filename to String cannot fail");
    }
    filename.push_str(".lifecycle.lock");
    filename
}

/// A set of lifecycle locks acquired in lexical name order. Taking the locks
/// in a fixed order lets operations that affect two instances (such as rename
/// and clone) run concurrently with unrelated instances without deadlocking.
pub(crate) struct LifecycleLocks {
    _locks: Vec<LifecycleLock>,
}

impl LifecycleLocks {
    pub(crate) fn acquire(paths: &Paths, game: GameId, names: &[&str]) -> Result<Self> {
        let mut names = names.to_vec();
        names.sort_unstable();
        names.dedup();

        let mut locks = Vec::with_capacity(names.len());
        for name in names {
            locks.push(LifecycleLock::acquire(paths, game, name)?);
        }
        Ok(Self { _locks: locks })
    }
}

/// Renames an instance on disk and in its state, provided it isn't running
/// and no instance already exists under `new_name`. The world name (and thus
/// its save files) is left untouched — only the instance's own identity moves.
pub fn rename(paths: &Paths, db: &Db, old_name: &str, new_name: &str) -> Result<Instance> {
    match crate::game::instances::rename(paths, db, GameId::Valheim, old_name, new_name)? {
        crate::game::instances::GameInstance::Valheim(instance) => Ok(instance),
        _ => unreachable!("Valheim rename returned a different game"),
    }
}

/// Removes an instance's on-disk directory — optionally preserving its
/// `backups` subdirectory — and its row from the database. Shared by
/// `commands::delete::run` and `game::instances::delete`.
/// It locks and rechecks liveness itself so a concurrent start cannot race
/// between a caller's confirmation and deletion.
pub fn delete(paths: &Paths, db: &Db, name: &str, keep_backups: bool) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, GameId::Valheim, name)?;
    let instance = Instance::load_existing(paths, db, name)?;
    if crate::podman::valheim::is_running(paths, db, &instance)? {
        bail!(InstanceError::AlreadyRunning(name.to_string()));
    }

    delete_unlocked(db, &instance, keep_backups)
}

fn delete_unlocked(db: &Db, instance: &Instance, keep_backups: bool) -> Result<()> {
    delete_instance_dir(&instance.dir, keep_backups)?;
    crate::db::instances::delete(db, &instance.state.name)?;
    Ok(())
}

/// Deletes an instance directory while optionally retaining its backups.
/// Shared by game drivers because all current layouts place backups directly
/// under the instance root.
pub(crate) fn delete_instance_dir(
    instance_dir: &std::path::Path,
    keep_backups: bool,
) -> Result<()> {
    if keep_backups {
        let backups_dir = paths::instance_backups_dir(instance_dir);
        for entry in std::fs::read_dir(instance_dir)
            .with_context(|| format!("failed to read instance dir {}", instance_dir.display()))?
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
        std::fs::remove_dir_all(instance_dir)
            .with_context(|| format!("failed to remove instance dir {}", instance_dir.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::lists::{self, ListKind};
    use crate::instance::state::InstalledMod;
    use chrono::Utc;

    fn temp_paths(label: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!(
            "odin-lifecycle-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        }
    }

    #[test]
    fn lifecycle_lock_is_exclusive_per_instance_and_released_on_drop() {
        let paths = temp_paths("lock");
        let first = LifecycleLock::acquire(&paths, GameId::Valheim, "alpha").unwrap();
        let error = LifecycleLock::acquire(&paths, GameId::Valheim, "alpha").unwrap_err();
        assert!(matches!(
            error.downcast_ref::<InstanceError>(),
            Some(InstanceError::TransitionInProgress(name)) if name == "alpha"
        ));

        LifecycleLock::acquire(&paths, GameId::Valheim, "bravo").unwrap();
        drop(first);
        LifecycleLock::acquire(&paths, GameId::Valheim, "alpha").unwrap();
    }

    #[test]
    fn same_name_in_different_games_uses_independent_lifecycle_locks() {
        let paths = temp_paths("game-locks");
        let _valheim = LifecycleLock::acquire(&paths, GameId::Valheim, "shared").unwrap();

        LifecycleLock::acquire(&paths, GameId::Rust, "shared").unwrap();
    }

    #[test]
    fn lock_filename_is_a_safe_game_scoped_file_name() {
        let paths = temp_paths("filename");
        let valheim = lock_filename(&paths, GameId::Valheim, "shared");
        let rust = lock_filename(&paths, GameId::Rust, "shared");

        assert_ne!(valheim, rust);
        assert!(valheim.ends_with(".lifecycle.lock"));
        assert!(
            valheim
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.')
        );
    }

    #[test]
    fn rename_moves_directory_and_preserves_access_lists() {
        let paths = temp_paths("rename");
        let db = Db::open(&paths).unwrap();
        let mut instance = Instance::create(&paths, &db, "old-name").unwrap();
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "author-mod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
            pinned: false,
        });
        instance.save(&db).unwrap();
        let ids = vec!["76561197960287930".to_string()];
        lists::write(&db, &instance, ListKind::Admin, &ids).unwrap();

        let renamed = rename(&paths, &db, "old-name", "new-name").unwrap();

        assert!(!paths.instance_dir("old-name").exists());
        assert!(renamed.dir.exists());
        assert_eq!(renamed.state.installed_mods.len(), 1);
        assert_eq!(lists::read(&db, &renamed, ListKind::Admin).unwrap(), ids);
    }
}
