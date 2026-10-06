//! Game-owned instance operations used by the canonical multi-game API.
//!
//! The concrete configuration remains typed by game, but the lifecycle and
//! backup contract is shared here so callers do not need to know which
//! server implementation owns an instance.

use anyhow::{Context, Result};

use crate::backup::BackupEntry;
use crate::db::Db;
use crate::db::game_instances::{self, GenericGameInstance, RustInstance};
use crate::instance::{Instance, lifecycle};
use crate::paths::Paths;

use super::{GameId, generic, rust};

pub enum GameInstance {
    Valheim(Instance),
    Rust(RustInstance),
    Generic(GenericGameInstance),
}

fn generic_save_dir(paths: &Paths, instance: &GenericGameInstance) -> std::path::PathBuf {
    let root = paths.game_instance_dir(instance.identity.game, instance.name());
    match instance.identity.game {
        GameId::VRising => root.join("data/Saves"),
        GameId::Palworld => root.join("runtime/Pal/Saved/SaveGames"),
        GameId::RunescapeDragonwilds => root.join("runtime/RSDragonwilds/Saved/Savegames"),
        GameId::SevenDaysToDie => root.join("Saves"),
        GameId::Valheim | GameId::Rust => unreachable!(),
    }
}

pub fn create(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<GameInstance> {
    let defaults = crate::db::settings::instance_defaults(db)?;
    let instance = match game {
        GameId::Valheim => Instance::create(paths, db, name).map(GameInstance::Valheim),
        GameId::Rust => game_instances::create_rust(paths, db, name).map(GameInstance::Rust),
        GameId::VRising
        | GameId::Palworld
        | GameId::RunescapeDragonwilds
        | GameId::SevenDaysToDie => {
            game_instances::create_generic(paths, db, game, name).map(GameInstance::Generic)
        }
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
        GameInstance::Generic(mut instance) => {
            instance.config.auto_restart = defaults.auto_restart;
            game_instances::set_generic_auto_restart(db, game, name, defaults.auto_restart)?;
            GameInstance::Generic(instance)
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
        GameId::VRising
        | GameId::Palworld
        | GameId::RunescapeDragonwilds
        | GameId::SevenDaysToDie => game_instances::load_generic(db, game, name)?
            .map(GameInstance::Generic)
            .context("game instance does not exist"),
    }
}

/// Returns the authoritative liveness state for an instance regardless of
/// game. Used by background policy loops before they decide whether a
/// lifecycle operation is needed.
pub fn is_running(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<bool> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(instance) => lifecycle::is_running(&instance),
        GameInstance::Rust(instance) => Ok(instance.is_running()),
        GameInstance::Generic(instance) => Ok(instance.is_running()),
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
        GameId::VRising
        | GameId::Palworld
        | GameId::RunescapeDragonwilds
        | GameId::SevenDaysToDie => generic::start(paths, db, game, name)
            .await
            .map(GameInstance::Generic),
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
        GameId::VRising
        | GameId::Palworld
        | GameId::RunescapeDragonwilds
        | GameId::SevenDaysToDie => generic::stop(paths, db, game, name).await,
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
        GameId::VRising
        | GameId::Palworld
        | GameId::RunescapeDragonwilds
        | GameId::SevenDaysToDie => {
            if is_running(paths, db, game, name)? {
                generic::stop(paths, db, game, name).await?;
            }
            generic::start(paths, db, game, name)
                .await
                .map(GameInstance::Generic)
        }
    }
}

pub fn delete(paths: &Paths, db: &Db, game: GameId, name: &str, keep_backups: bool) -> Result<()> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(instance) => lifecycle::delete(db, &instance, keep_backups),
        GameInstance::Rust(instance) => rust::delete(paths, db, &instance, keep_backups),
        GameInstance::Generic(instance) => {
            if instance.identity.game == GameId::SevenDaysToDie && instance.is_running() {
                anyhow::bail!("7 Days to Die backups require a stopped server");
            }
            anyhow::ensure!(
                !instance.is_running(),
                crate::instance::InstanceError::AlreadyRunning(instance.identity.name)
            );
            crate::instance::lifecycle::delete_instance_dir(
                &paths.game_instance_dir(instance.identity.game, instance.name()),
                keep_backups,
            )?;
            game_instances::delete_generic(db, instance.identity.game, instance.name())
        }
    }
}

pub fn list_backups(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<Vec<BackupEntry>> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(instance) => crate::backup::list(db, &instance.state.name),
        GameInstance::Rust(instance) => rust::list_backups(paths, db, &instance),
        GameInstance::Generic(instance) => {
            crate::db::backups::list_for_game(db, instance.identity.game, instance.name())
        }
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
        GameInstance::Generic(instance) => {
            // A filesystem copy made while Palworld is changing its world is
            // not a useful backup. Its local REST endpoint owns the save
            // operation, so ask it to flush before taking the snapshot.
            if instance.identity.game == GameId::Palworld && instance.is_running() {
                crate::game::palworld::save(&instance)?;
            }
            crate::backup::create_at(
                db,
                instance.identity.game,
                instance.name(),
                &paths.game_instance_dir(instance.identity.game, instance.name()),
                &generic_save_dir(paths, &instance),
            )
        }
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
        GameInstance::Generic(instance) => {
            anyhow::ensure!(
                !instance.is_running(),
                crate::instance::InstanceError::AlreadyRunning(instance.identity.name.clone())
            );
            crate::backup::restore_at(
                db,
                instance.identity.game,
                instance.name(),
                &paths.game_instance_dir(instance.identity.game, instance.name()),
                &generic_save_dir(paths, &instance),
                backup_id,
            )
        }
    }
}

pub fn delete_backup(paths: &Paths, db: &Db, game: GameId, name: &str, id: &str) -> Result<()> {
    let _lock = lifecycle::LifecycleLock::acquire(paths, game, name)?;
    load(paths, db, game, name)?;
    list_backups(paths, db, game, name)?;
    crate::backup::delete_at(db, game, name, &paths.game_instance_dir(game, name), id)
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
        GameInstance::Generic(i) => i.is_running(),
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
    config.rcon_port = cloned.config.rcon_port;
    config.rcon_password = cloned.config.rcon_password;
    config.hostname = target.into();
    if let Err(error) = game_instances::update_rust_config(db, target, &config) {
        game_instances::delete_rust(db, target)?;
        std::fs::remove_dir_all(paths.game_instance_dir(GameId::Rust, target))?;
        return Err(error);
    }
    game_instances::load_rust(db, target)?.context("cloned Rust instance disappeared")
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
                GameInstance::Generic(instance) => assert!(instance.config.auto_restart),
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
        assert_ne!(original.config.rcon_port, cloned.config.rcon_port);
        assert_ne!(original.config.rcon_password, cloned.config.rcon_password);
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
