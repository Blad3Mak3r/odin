//! Rust Dedicated's persisted owner, moderator, and ban lists.
//!
//! Rust loads these files from `server/<identity>/cfg/` when it starts.  We
//! therefore only change them while the instance is stopped, preventing Rust
//! from overwriting an edit when it later persists its in-memory state.

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use thiserror::Error;

use crate::db::game_instances::RustInstance;
use crate::game::GameId;
use crate::instance::lifecycle::LifecycleLock;
use crate::paths::Paths;

#[derive(Debug, Error)]
pub enum RustAccessListError {
    #[error(
        "'{0}' is not a valid Rust access-list kind; expected 'owner', 'moderator', or 'banned'"
    )]
    UnknownKind(String),
    #[error("'{0}' is not a valid SteamID64: expected exactly 17 digits")]
    WrongIdLength(String),
    #[error("'{0}' is not a valid SteamID64: expected it to start with '7656119'")]
    WrongIdPrefix(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustAccessListKind {
    Owner,
    Moderator,
    Banned,
}

impl RustAccessListKind {
    pub fn parse(raw: &str) -> Result<Self, RustAccessListError> {
        match raw {
            "owner" => Ok(Self::Owner),
            "moderator" => Ok(Self::Moderator),
            "banned" => Ok(Self::Banned),
            other => Err(RustAccessListError::UnknownKind(other.to_string())),
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::Owner => "ownerid",
            Self::Moderator => "moderatorid",
            Self::Banned => "banid",
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::Owner | Self::Moderator => "users.cfg",
            Self::Banned => "bans.cfg",
        }
    }
}

pub fn config_dir(paths: &Paths, instance: &RustInstance) -> PathBuf {
    paths
        .game_install_dir(GameId::Rust)
        .join("server")
        .join(&instance.identity.id)
        .join("cfg")
}

fn list_path(paths: &Paths, instance: &RustInstance, kind: RustAccessListKind) -> PathBuf {
    config_dir(paths, instance).join(kind.filename())
}

/// Reads the SteamID64s managed by one Rust access-list command. Malformed
/// hand-written commands are left alone and not shown as usable entries.
pub fn read(
    paths: &Paths,
    instance: &RustInstance,
    kind: RustAccessListKind,
) -> Result<Vec<String>> {
    let path = list_path(paths, instance, kind);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };

    let mut ids = contents
        .lines()
        .filter_map(|line| command_and_id(line, kind.command()))
        .filter(|id| validate_steam_id64(id).is_ok())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Replaces entries of the requested kind while preserving comments and
/// commands of the other kind in the same `users.cfg` file.
pub fn write(
    paths: &Paths,
    instance: &RustInstance,
    kind: RustAccessListKind,
    ids: &[String],
) -> Result<()> {
    let _lock = LifecycleLock::acquire(paths, GameId::Rust, instance.name())?;
    write_unlocked(paths, instance, kind, ids)
}

fn write_unlocked(
    paths: &Paths,
    instance: &RustInstance,
    kind: RustAccessListKind,
    ids: &[String],
) -> Result<()> {
    ensure!(
        !instance.is_running(),
        crate::instance::InstanceError::AlreadyRunning(instance.name().to_string())
    );
    for id in ids {
        validate_steam_id64(id)?;
    }

    let path = list_path(paths, instance, kind);
    let existing = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let mut lines = existing
        .lines()
        .filter(|line| command_and_id(line, kind.command()).is_none())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for id in ids {
        lines.push(format!("{} {id} \"\" \"\"", kind.command()));
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let contents = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    let temp_path = path.with_extension(format!("cfg.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temp_path, contents)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    std::fs::rename(&temp_path, &path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            temp_path.display(),
            path.display()
        )
    })
}

pub fn add_id(
    paths: &Paths,
    instance: &RustInstance,
    kind: RustAccessListKind,
    id: &str,
) -> Result<bool> {
    let _lock = LifecycleLock::acquire(paths, GameId::Rust, instance.name())?;
    validate_steam_id64(id)?;
    let mut ids = read(paths, instance, kind)?;
    if ids.iter().any(|existing| existing == id) {
        return Ok(false);
    }
    ids.push(id.to_string());
    write_unlocked(paths, instance, kind, &ids)?;
    Ok(true)
}

pub fn remove_id(
    paths: &Paths,
    instance: &RustInstance,
    kind: RustAccessListKind,
    id: &str,
) -> Result<bool> {
    let _lock = LifecycleLock::acquire(paths, GameId::Rust, instance.name())?;
    let mut ids = read(paths, instance, kind)?;
    let original_len = ids.len();
    ids.retain(|existing| existing != id);
    if ids.len() == original_len {
        return Ok(false);
    }
    write_unlocked(paths, instance, kind, &ids)?;
    Ok(true)
}

fn command_and_id<'a>(line: &'a str, command: &str) -> Option<&'a str> {
    let mut words = line.split_whitespace();
    if words.next()? != command {
        return None;
    }
    Some(words.next()?.trim_matches('"'))
}

fn validate_steam_id64(id: &str) -> Result<(), RustAccessListError> {
    if id.len() != 17 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(RustAccessListError::WrongIdLength(id.to_string()));
    }
    if !id.starts_with("7656119") {
        return Err(RustAccessListError::WrongIdPrefix(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, game_instances};

    const ID_A: &str = "76561197960287930";
    const ID_B: &str = "76561197960287931";

    fn temp_paths(label: &str) -> Paths {
        let root = std::env::temp_dir().join(format!(
            "odin-rust-access-lists-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        Paths {
            data_dir: root.clone(),
            config_dir: root,
        }
    }

    fn instance(paths: &Paths) -> (Db, RustInstance) {
        let db = Db::open(paths).unwrap();
        let instance = game_instances::create_rust(paths, &db, "my-server").unwrap();
        (db, instance)
    }

    #[test]
    fn writes_owners_and_preserves_moderators_in_users_cfg() {
        let paths = temp_paths("users");
        let (_db, instance) = instance(&paths);
        write(
            &paths,
            &instance,
            RustAccessListKind::Owner,
            &[ID_A.to_string()],
        )
        .unwrap();
        write(
            &paths,
            &instance,
            RustAccessListKind::Moderator,
            &[ID_B.to_string()],
        )
        .unwrap();

        assert_eq!(
            read(&paths, &instance, RustAccessListKind::Owner).unwrap(),
            vec![ID_A.to_string()]
        );
        assert_eq!(
            read(&paths, &instance, RustAccessListKind::Moderator).unwrap(),
            vec![ID_B.to_string()]
        );
        let file =
            std::fs::read_to_string(config_dir(&paths, &instance).join("users.cfg")).unwrap();
        assert!(file.contains(&format!("ownerid {ID_A}")));
        assert!(file.contains(&format!("moderatorid {ID_B}")));
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn adds_and_removes_bans() {
        let paths = temp_paths("bans");
        let (_db, instance) = instance(&paths);
        assert!(add_id(&paths, &instance, RustAccessListKind::Banned, ID_A).unwrap());
        assert!(!add_id(&paths, &instance, RustAccessListKind::Banned, ID_A).unwrap());
        assert!(remove_id(&paths, &instance, RustAccessListKind::Banned, ID_A).unwrap());
        assert!(
            read(&paths, &instance, RustAccessListKind::Banned)
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn rejects_invalid_ids_before_touching_the_file() {
        let paths = temp_paths("invalid");
        let (_db, instance) = instance(&paths);
        let error = write(
            &paths,
            &instance,
            RustAccessListKind::Owner,
            &["not-a-steamid".to_string()],
        )
        .unwrap_err();
        assert!(error.to_string().contains("not a valid SteamID64"));
        assert!(!config_dir(&paths, &instance).join("users.cfg").exists());
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }
}
