//! Valheim's three access-control files: `adminlist.txt`, `bannedlist.txt`,
//! and `permittedlist.txt`. Each is a plain text file, one SteamID64 per
//! line, that Valheim reads directly from the world save directory at
//! startup — no server flag or `run.sh` change is needed for it to take
//! effect, and a missing file simply means "no entries" to Valheim.
//!
//! The database is the source of truth: every write goes to the
//! `access_list_entries` table first, then regenerates the on-disk file
//! from those rows, so a hand-edit to the file is silently overwritten by
//! the next change made through Odin.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;

use super::Instance;
use crate::db::Db;
use crate::paths;

/// Validation failures a caller (e.g. the web API) may want to distinguish
/// from other, unexpected errors — these always mean "the input was bad",
/// never "something went wrong on our end".
#[derive(Debug, Error)]
pub enum ListsError {
    #[error("'{0}' is not a valid list kind; expected 'admin', 'banned', or 'permitted'")]
    UnknownKind(String),
    #[error("'{0}' is not a valid SteamID64: expected exactly 17 digits")]
    WrongIdLength(String),
    #[error("'{0}' is not a valid SteamID64: expected it to start with '7656119'")]
    WrongIdPrefix(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Admin,
    Banned,
    Permitted,
}

impl ListKind {
    pub fn filename(self) -> &'static str {
        match self {
            ListKind::Admin => "adminlist.txt",
            ListKind::Banned => "bannedlist.txt",
            ListKind::Permitted => "permittedlist.txt",
        }
    }

    pub(crate) fn db_value(self) -> &'static str {
        match self {
            ListKind::Admin => "admin",
            ListKind::Banned => "banned",
            ListKind::Permitted => "permitted",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, ListsError> {
        match raw {
            "admin" => Ok(ListKind::Admin),
            "banned" => Ok(ListKind::Banned),
            "permitted" => Ok(ListKind::Permitted),
            other => Err(ListsError::UnknownKind(other.to_string())),
        }
    }
}

pub fn list_path(instance_dir: &Path, kind: ListKind) -> PathBuf {
    paths::instance_saves_dir(instance_dir).join(kind.filename())
}

/// Reads the ids currently in a list, from the database.
pub fn read(db: &Db, instance: &Instance, kind: ListKind) -> Result<Vec<String>> {
    crate::db::lists::read(db, &instance.state.name, kind.db_value())
}

/// Replaces a list's contents wholesale: every id is validated before
/// anything is written, so a bad entry never partially clobbers the
/// database or the file. Updates the database first, then regenerates the
/// on-disk file from it atomically (temp file + rename).
pub fn write(db: &Db, instance: &Instance, kind: ListKind, ids: &[String]) -> Result<()> {
    for id in ids {
        validate_steam_id64(id)?;
    }

    crate::db::lists::replace(db, &instance.state.name, kind.db_value(), ids)?;

    write_file(&instance.dir, kind, ids)
}

fn write_file(instance_dir: &Path, kind: ListKind, ids: &[String]) -> Result<()> {
    let path = list_path(instance_dir, kind);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut contents = ids.join("\n");
    if !ids.is_empty() {
        contents.push('\n');
    }

    let tmp_path = path.with_extension("txt.tmp");
    std::fs::write(&tmp_path, contents)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, &path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            tmp_path.display(),
            path.display()
        )
    })
}

/// Adds an id if it isn't already present. Returns `false` (no-op) if it was
/// already in the list.
pub fn add_id(db: &Db, instance: &Instance, kind: ListKind, id: &str) -> Result<bool> {
    validate_steam_id64(id)?;

    let mut ids = read(db, instance, kind)?;
    if ids.iter().any(|existing| existing == id) {
        return Ok(false);
    }
    ids.push(id.to_string());
    write(db, instance, kind, &ids)?;
    Ok(true)
}

/// Removes an id if present. Returns `false` (no-op) if it wasn't there.
pub fn remove_id(db: &Db, instance: &Instance, kind: ListKind, id: &str) -> Result<bool> {
    let mut ids = read(db, instance, kind)?;
    let original_len = ids.len();
    ids.retain(|existing| existing != id);
    if ids.len() == original_len {
        return Ok(false);
    }
    write(db, instance, kind, &ids)?;
    Ok(true)
}

/// Reads a list directly from its on-disk file, bypassing the database —
/// used only by the bootstrap importer to seed the database from an
/// existing installation. A missing file reads as empty, matching how
/// Valheim itself treats it.
pub(crate) fn read_from_disk(instance_dir: &Path, kind: ListKind) -> Result<Vec<String>> {
    let path = list_path(instance_dir, kind);
    match std::fs::read_to_string(&path) {
        Ok(raw) => Ok(raw
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// A SteamID64 is always a 17-digit number starting with the fixed `7656119`
/// prefix every Steam64 account id shares; this rejects pasted SteamID3s,
/// vanity URLs, or other garbage with a clear error instead of writing it to
/// a file Valheim will just silently ignore.
pub(crate) fn validate_steam_id64(id: &str) -> Result<(), ListsError> {
    if id.len() != 17 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ListsError::WrongIdLength(id.to_string()));
    }
    if !id.starts_with("7656119") {
        return Err(ListsError::WrongIdPrefix(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::state::InstanceState;

    fn temp_db_and_instance(label: &str) -> (Db, Instance) {
        let dir = std::env::temp_dir().join(format!(
            "odin-lists-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = crate::paths::Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let db = Db::open(&paths).unwrap();
        let state = InstanceState::new("my-server", 2456);
        crate::db::instances::save(&db, &state).unwrap();
        let instance = Instance {
            dir: paths.instance_dir(&state.name),
            state,
        };
        (db, instance)
    }

    const VALID_ID_A: &str = "76561197960287930";
    const VALID_ID_B: &str = "76561197960287931";

    #[test]
    fn missing_list_reads_as_empty() {
        let (db, instance) = temp_db_and_instance("missing");
        assert_eq!(
            read(&db, &instance, ListKind::Admin).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn write_then_read_round_trips() {
        let (db, instance) = temp_db_and_instance("roundtrip");
        let ids = vec![VALID_ID_A.to_string(), VALID_ID_B.to_string()];
        write(&db, &instance, ListKind::Banned, &ids).unwrap();
        assert_eq!(read(&db, &instance, ListKind::Banned).unwrap(), ids);
    }

    #[test]
    fn write_regenerates_the_file_to_match_the_database() {
        let (db, instance) = temp_db_and_instance("write-through");
        let ids = vec![VALID_ID_A.to_string()];
        write(&db, &instance, ListKind::Permitted, &ids).unwrap();

        let on_disk = read_from_disk(&instance.dir, ListKind::Permitted).unwrap();
        assert_eq!(on_disk, ids);
    }

    #[test]
    fn write_overwrites_a_manually_edited_file() {
        let (db, instance) = temp_db_and_instance("db-wins");
        write(&db, &instance, ListKind::Admin, &[VALID_ID_A.to_string()]).unwrap();

        // Simulate a hand-edit made outside Odin.
        std::fs::write(list_path(&instance.dir, ListKind::Admin), "not-a-steamid\n").unwrap();

        // The next write through Odin regenerates the file from the database.
        write(
            &db,
            &instance,
            ListKind::Admin,
            &[VALID_ID_A.to_string(), VALID_ID_B.to_string()],
        )
        .unwrap();

        let on_disk = read_from_disk(&instance.dir, ListKind::Admin).unwrap();
        assert_eq!(
            on_disk,
            vec![VALID_ID_A.to_string(), VALID_ID_B.to_string()]
        );
    }

    #[test]
    fn write_rejects_invalid_id() {
        let (db, instance) = temp_db_and_instance("invalid");
        let err = write(
            &db,
            &instance,
            ListKind::Admin,
            &["not-a-steamid".to_string()],
        )
        .unwrap_err();
        assert!(err.to_string().contains("not a valid SteamID64"));
        assert!(read(&db, &instance, ListKind::Admin).unwrap().is_empty());
    }

    #[test]
    fn add_id_is_idempotent() {
        let (db, instance) = temp_db_and_instance("add-idempotent");
        assert!(add_id(&db, &instance, ListKind::Permitted, VALID_ID_A).unwrap());
        assert!(!add_id(&db, &instance, ListKind::Permitted, VALID_ID_A).unwrap());
        assert_eq!(
            read(&db, &instance, ListKind::Permitted).unwrap(),
            vec![VALID_ID_A.to_string()]
        );
    }

    #[test]
    fn remove_id_is_idempotent() {
        let (db, instance) = temp_db_and_instance("remove-idempotent");
        add_id(&db, &instance, ListKind::Admin, VALID_ID_A).unwrap();
        assert!(remove_id(&db, &instance, ListKind::Admin, VALID_ID_A).unwrap());
        assert!(!remove_id(&db, &instance, ListKind::Admin, VALID_ID_A).unwrap());
        assert_eq!(
            read(&db, &instance, ListKind::Admin).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn parses_valid_kinds_only() {
        assert!(matches!(ListKind::parse("admin"), Ok(ListKind::Admin)));
        assert!(matches!(ListKind::parse("banned"), Ok(ListKind::Banned)));
        assert!(matches!(
            ListKind::parse("permitted"),
            Ok(ListKind::Permitted)
        ));
        assert!(ListKind::parse("bogus").is_err());
    }
}
