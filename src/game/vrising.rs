//! V Rising's administrator and ban files live below the instance-specific
//! persistent-data path selected by Odin's `-persistentDataPath` argument.

use std::path::PathBuf;

use anyhow::{Context, Result};
use thiserror::Error;

use crate::paths::Paths;

#[derive(Debug, Error)]
pub enum VRisingAccessListError {
    #[error("'{0}' is not a valid V Rising access-list kind; expected 'admin' or 'banned'")]
    UnknownKind(String),
    #[error("'{0}' is not a valid SteamID64: expected exactly 17 digits")]
    WrongIdLength(String),
    #[error("'{0}' is not a valid SteamID64: expected it to start with '7656119'")]
    WrongIdPrefix(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VRisingAccessListKind {
    Admin,
    Banned,
}

impl VRisingAccessListKind {
    pub fn parse(raw: &str) -> Result<Self, VRisingAccessListError> {
        match raw {
            "admin" => Ok(Self::Admin),
            "banned" => Ok(Self::Banned),
            other => Err(VRisingAccessListError::UnknownKind(other.to_string())),
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::Admin => "adminlist.txt",
            Self::Banned => "banlist.txt",
        }
    }
}

fn list_path(paths: &Paths, name: &str, kind: VRisingAccessListKind) -> PathBuf {
    paths
        .game_instance_dir(crate::game::GameId::VRising, name)
        .join("data/Settings")
        .join(kind.filename())
}

pub fn read(paths: &Paths, name: &str, kind: VRisingAccessListKind) -> Result<Vec<String>> {
    let path = list_path(paths, name, kind);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let mut ids = contents
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .filter(|id| validate_steam_id64(id).is_ok())
        .map(str::to_string)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

pub fn write(paths: &Paths, name: &str, kind: VRisingAccessListKind, ids: &[String]) -> Result<()> {
    for id in ids {
        validate_steam_id64(id)?;
    }
    let mut ids = ids.to_vec();
    ids.sort();
    ids.dedup();
    let path = list_path(paths, name, kind);
    let parent = path.parent().expect("access list has a parent directory");
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let contents = if ids.is_empty() {
        String::new()
    } else {
        format!("{}\n", ids.join("\n"))
    };
    let temporary = path.with_extension(format!("txt.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, contents)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    std::fs::rename(&temporary, &path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            temporary.display(),
            path.display()
        )
    })
}

pub fn add_id(paths: &Paths, name: &str, kind: VRisingAccessListKind, id: &str) -> Result<bool> {
    validate_steam_id64(id)?;
    let mut ids = read(paths, name, kind)?;
    if ids.iter().any(|current| current == id) {
        return Ok(false);
    }
    ids.push(id.to_string());
    write(paths, name, kind, &ids)?;
    Ok(true)
}

pub fn remove_id(paths: &Paths, name: &str, kind: VRisingAccessListKind, id: &str) -> Result<bool> {
    let mut ids = read(paths, name, kind)?;
    let previous_len = ids.len();
    ids.retain(|current| current != id);
    if ids.len() == previous_len {
        return Ok(false);
    }
    write(paths, name, kind, &ids)?;
    Ok(true)
}

fn validate_steam_id64(id: &str) -> Result<(), VRisingAccessListError> {
    if id.len() != 17 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(VRisingAccessListError::WrongIdLength(id.to_string()));
    }
    if !id.starts_with("7656119") {
        return Err(VRisingAccessListError::WrongIdPrefix(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_lists_round_trip_under_the_isolated_data_directory() {
        let dir = std::env::temp_dir().join(format!("odin-vrising-lists-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let ids = vec![
            "76561197960287931".to_string(),
            "76561197960287930".to_string(),
        ];

        write(&paths, "vrising", VRisingAccessListKind::Admin, &ids).unwrap();

        assert_eq!(
            read(&paths, "vrising", VRisingAccessListKind::Admin).unwrap(),
            vec!["76561197960287930", "76561197960287931"]
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
