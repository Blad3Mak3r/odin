//! Game-owned instance operations used by the canonical multi-game API.
//!
//! The concrete configuration remains typed by game, but the lifecycle and
//! backup contract is shared here so callers do not need to know which
//! server implementation owns an instance.

use anyhow::{Context, Result};

use crate::backup::BackupEntry;
use crate::db::Db;
use crate::db::game_instances::{self, RustInstance};
use crate::instance::{Instance, lifecycle};
use crate::paths::Paths;

use super::{GameId, rust};

pub enum GameInstance {
    Valheim(Instance),
    Rust(RustInstance),
}

pub fn create(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<GameInstance> {
    let defaults = crate::db::settings::instance_defaults(db)?;
    let instance = match game {
        GameId::Valheim => Instance::create(paths, db, name).map(GameInstance::Valheim),
        GameId::Rust => game_instances::create_rust(paths, db, name).map(GameInstance::Rust),
    }?;
    let instance = match instance {
        GameInstance::Valheim(mut instance) => {
            instance.state.auto_restart = defaults.auto_restart;
            instance.save(db)?;
            GameInstance::Valheim(instance)
        }
        GameInstance::Rust(instance) => {
            let mut config = instance.config.clone();
            config.auto_restart = defaults.auto_restart;
            GameInstance::Rust(game_instances::update_rust_config(db, name, &config)?)
        }
    };
    crate::db::backup_schedules::upsert_for_game(
        db,
        game,
        name,
        defaults.backup_interval_hours,
        defaults.backup_retain_count,
        defaults.backup_enabled,
    )?;
    Ok(instance)
}

pub fn load(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<GameInstance> {
    match game {
        GameId::Valheim => Instance::load_existing(paths, db, name).map(GameInstance::Valheim),
        GameId::Rust => game_instances::load_rust(db, name)?
            .map(GameInstance::Rust)
            .context("Rust instance does not exist"),
    }
}

pub async fn start(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<GameInstance> {
    match game {
        GameId::Valheim => lifecycle::start(paths, db, name)
            .await
            .map(GameInstance::Valheim),
        GameId::Rust => {
            let instance = load(paths, db, game, name)?;
            let GameInstance::Rust(instance) = instance else {
                unreachable!("Rust game must load a Rust instance")
            };
            rust::start(paths, db, &instance)
                .await
                .map(GameInstance::Rust)
        }
    }
}

pub async fn stop(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<()> {
    match game {
        GameId::Valheim => lifecycle::stop(paths, db, name).await,
        GameId::Rust => {
            let instance = load(paths, db, game, name)?;
            let GameInstance::Rust(instance) = instance else {
                unreachable!("Rust game must load a Rust instance")
            };
            rust::stop(paths, db, &instance).await
        }
    }
}

pub async fn restart(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<GameInstance> {
    match game {
        GameId::Valheim => lifecycle::restart(paths, db, name)
            .await
            .map(GameInstance::Valheim),
        GameId::Rust => {
            let instance = load(paths, db, game, name)?;
            let GameInstance::Rust(instance) = instance else {
                unreachable!("Rust game must load a Rust instance")
            };
            rust::restart(paths, db, &instance)
                .await
                .map(GameInstance::Rust)
        }
    }
}

pub fn delete(paths: &Paths, db: &Db, game: GameId, name: &str, keep_backups: bool) -> Result<()> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(instance) => lifecycle::delete(db, &instance, keep_backups),
        GameInstance::Rust(instance) => rust::delete(paths, db, &instance, keep_backups),
    }
}

pub fn list_backups(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<Vec<BackupEntry>> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(instance) => crate::backup::list(db, &instance.state.name),
        GameInstance::Rust(instance) => rust::list_backups(paths, db, &instance),
    }
}

pub fn create_backup(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<BackupEntry> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(_) => {
            let _lock = lifecycle::LifecycleLock::acquire(paths, game, name)?;
            let instance = Instance::load_existing(paths, db, name)?;
            crate::backup::create(&instance, db)
        }
        GameInstance::Rust(instance) => rust::create_backup(paths, db, &instance),
    }
}

pub fn restore_backup(
    paths: &Paths,
    db: &Db,
    game: GameId,
    name: &str,
    backup_id: &str,
) -> Result<()> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(_) => {
            let _lock = lifecycle::LifecycleLock::acquire(paths, game, name)?;
            let instance = Instance::load_existing(paths, db, name)?;
            crate::backup::restore(&instance, db, backup_id)
        }
        GameInstance::Rust(instance) => rust::restore_backup(paths, db, &instance, backup_id),
    }
}

pub fn delete_backup(paths: &Paths, db: &Db, game: GameId, name: &str, id: &str) -> Result<()> {
    let _lock = lifecycle::LifecycleLock::acquire(paths, game, name)?;
    load(paths, db, game, name)?;
    list_backups(paths, db, game, name)?;
    crate::backup::delete_at(db, game, name, &paths.game_instance_dir(game, name), id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[test]
    fn create_applies_global_defaults_to_each_game() {
        let root =
            std::env::temp_dir().join(format!("odin-game-defaults-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: root.clone(),
            config_dir: root.clone(),
        };
        let db = Db::open(&paths).unwrap();
        crate::db::settings::set_instance_defaults(
            &db,
            &crate::db::settings::InstanceDefaults {
                auto_restart: true,
                backup_enabled: true,
                backup_interval_hours: 8,
                backup_retain_count: 5,
            },
        )
        .unwrap();

        for game in [GameId::Valheim, GameId::Rust] {
            let instance = create(&paths, &db, game, game.as_str()).unwrap();
            match instance {
                GameInstance::Valheim(instance) => assert!(instance.state.auto_restart),
                GameInstance::Rust(instance) => assert!(instance.config.auto_restart),
            }
            let schedule = crate::db::backup_schedules::get_for_game(&db, game, game.as_str())
                .unwrap()
                .unwrap();
            assert!(schedule.enabled);
            assert_eq!(schedule.interval_hours, 8);
            assert_eq!(schedule.retain_count, 5);
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_preserves_identity_backups_and_other_games() {
        let root = std::env::temp_dir().join(format!("odin-rename-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: root.clone(),
            config_dir: root.clone(),
        };
        let db = Db::open(&paths).unwrap();
        create(&paths, &db, GameId::Valheim, "source").unwrap();
        create(&paths, &db, GameId::Rust, "source").unwrap();
        for game in [GameId::Valheim, GameId::Rust] {
            let id = game_instances::identity(&db, game, "source")
                .unwrap()
                .unwrap()
                .id;
            let backup = create_backup(&paths, &db, game, "source").unwrap();
            crate::db::backup_schedules::upsert_for_game(&db, game, "source", 24, 7, true).unwrap();
            rename(&paths, &db, game, "source", "target").unwrap();
            assert_eq!(
                game_instances::identity(&db, game, "target")
                    .unwrap()
                    .unwrap()
                    .id,
                id
            );
            assert_eq!(
                list_backups(&paths, &db, game, "target").unwrap()[0].id,
                backup.id
            );
            assert!(
                crate::db::backup_schedules::get_for_game(&db, game, "target")
                    .unwrap()
                    .unwrap()
                    .enabled
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rust_clone_has_new_identity_and_ports_without_world_or_credentials() {
        let root = std::env::temp_dir().join(format!("odin-clone-rust-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: root.clone(),
            config_dir: root.clone(),
        };
        let db = Db::open(&paths).unwrap();
        let original = game_instances::create_rust(&paths, &db, "source").unwrap();
        let cloned = clone_rust(&paths, &db, "source", "target").unwrap();
        assert_ne!(original.identity.id, cloned.identity.id);
        assert_ne!(original.config.port, cloned.config.port);
        assert_eq!(original.config.seed, cloned.config.seed);
        assert!(cloned.pid.is_none());
        assert!(!rust::backup_source(&paths, &cloned).exists());
        assert!(
            crate::db::backup_storage::get_for_game(&db, GameId::Rust, "target")
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loading_same_name_keeps_each_games_typed_instance() {
        let dir =
            std::env::temp_dir().join(format!("odin-game-operations-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        create(&paths, &db, GameId::Valheim, "shared").unwrap();
        create(&paths, &db, GameId::Rust, "shared").unwrap();

        assert!(matches!(
            load(&paths, &db, GameId::Valheim, "shared").unwrap(),
            GameInstance::Valheim(_)
        ));
        assert!(matches!(
            load(&paths, &db, GameId::Rust, "shared").unwrap(),
            GameInstance::Rust(_)
        ));
    }

    #[test]
    fn deleting_rust_keeps_a_same_named_valheim_instance() {
        let dir = std::env::temp_dir().join(format!("odin-game-delete-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        create(&paths, &db, GameId::Valheim, "shared").unwrap();
        create(&paths, &db, GameId::Rust, "shared").unwrap();

        delete(&paths, &db, GameId::Rust, "shared", false).unwrap();

        assert!(load(&paths, &db, GameId::Valheim, "shared").is_ok());
        assert!(
            crate::db::game_instances::load_rust(&db, "shared")
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}

pub fn rename(paths: &Paths, db: &Db, game: GameId, old: &str, new: &str) -> Result<GameInstance> {
    crate::cli::validate_instance_name(new).map_err(crate::instance::InstanceError::InvalidName)?;
    anyhow::ensure!(
        old != new,
        crate::instance::InstanceError::InvalidName("Choose a different name".into())
    );
    let _source_lock = lifecycle::LifecycleLock::acquire(paths, game, old)?;
    let _target_lock = lifecycle::LifecycleLock::acquire(paths, game, new)?;
    let instance = load(paths, db, game, old)?;
    let running = match &instance {
        GameInstance::Valheim(i) => lifecycle::is_running(i)?,
        GameInstance::Rust(i) => i.is_running(),
    };
    anyhow::ensure!(
        !running,
        crate::instance::InstanceError::AlreadyRunning(old.into())
    );
    anyhow::ensure!(
        game_instances::identity(db, game, new)?.is_none(),
        crate::instance::InstanceError::AlreadyExists(new.into())
    );
    let source = paths.game_instance_dir(game, old);
    let target = paths.game_instance_dir(game, new);
    anyhow::ensure!(!target.exists(), "destination directory already exists");
    std::fs::rename(&source, &target)?;
    if let Err(error) = game_instances::rename(db, game, old, new) {
        std::fs::rename(&target, &source)
            .context("could not recover original directory after rename failure")?;
        return Err(error);
    }
    load(paths, db, game, new)
}

pub fn clone_rust(paths: &Paths, db: &Db, source: &str, target: &str) -> Result<RustInstance> {
    let _source_lock = lifecycle::LifecycleLock::acquire(paths, GameId::Rust, source)?;
    let _target_lock = lifecycle::LifecycleLock::acquire(paths, GameId::Rust, target)?;
    let original =
        game_instances::load_rust(db, source)?.context("Rust instance does not exist")?;
    let cloned = game_instances::create_rust(paths, db, target)?;
    let mut config = original.config;
    config.port = cloned.config.port;
    config.query_port = cloned.config.query_port;
    config.hostname = target.into();
    if let Err(error) = game_instances::update_rust_config(db, target, &config) {
        game_instances::delete_rust(db, target)?;
        std::fs::remove_dir_all(paths.game_instance_dir(GameId::Rust, target))?;
        return Err(error);
    }
    game_instances::load_rust(db, target)?.context("cloned Rust instance disappeared")
}
