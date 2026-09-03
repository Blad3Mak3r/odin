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
    match game {
        GameId::Valheim => Instance::create(paths, db, name).map(GameInstance::Valheim),
        GameId::Rust => game_instances::create_rust(paths, db, name).map(GameInstance::Rust),
    }
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

pub fn list_backups(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<Vec<BackupEntry>> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(instance) => crate::backup::list(db, &instance.state.name),
        GameInstance::Rust(instance) => rust::list_backups(paths, &instance),
    }
}

pub fn create_backup(paths: &Paths, db: &Db, game: GameId, name: &str) -> Result<BackupEntry> {
    match load(paths, db, game, name)? {
        GameInstance::Valheim(instance) => crate::backup::create(&instance, db),
        GameInstance::Rust(instance) => rust::create_backup(paths, &instance),
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
        GameInstance::Valheim(instance) => crate::backup::restore(&instance, db, backup_id),
        GameInstance::Rust(instance) => rust::restore_backup(paths, &instance, backup_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

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
}
