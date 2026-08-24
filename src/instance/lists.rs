//! Valheim's three access-control files: `adminlist.txt`, `bannedlist.txt`,
//! and `permittedlist.txt`. Each is a plain text file, one SteamID64 per
//! line, that Valheim reads directly from the world save directory at
//! startup — no server flag or `run.sh` change is needed for it to take
//! effect, and a missing file simply means "no entries" to Valheim.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::paths;

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

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "admin" => Ok(ListKind::Admin),
            "banned" => Ok(ListKind::Banned),
            "permitted" => Ok(ListKind::Permitted),
            other => Err(format!(
                "'{other}' is not a valid list kind; expected 'admin', 'banned', or 'permitted'"
            )),
        }
    }
}

pub fn list_path(instance_dir: &Path, kind: ListKind) -> PathBuf {
    paths::instance_saves_dir(instance_dir).join(kind.filename())
}

/// Reads the ids currently in a list. A missing file isn't an error — it
/// just means the list is empty, matching how Valheim itself treats it.
pub fn read(instance_dir: &Path, kind: ListKind) -> Result<Vec<String>> {
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

/// Replaces a list's contents wholesale. Every id is validated before
/// anything is written, so a bad entry never partially clobbers the file.
/// Written atomically (temp file + rename), mirroring `InstanceState::save`.
pub fn write(instance_dir: &Path, kind: ListKind, ids: &[String]) -> Result<()> {
    for id in ids {
        validate_steam_id64(id).map_err(|e| anyhow::anyhow!(e))?;
    }

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
pub fn add_id(instance_dir: &Path, kind: ListKind, id: &str) -> Result<bool> {
    validate_steam_id64(id).map_err(|e| anyhow::anyhow!(e))?;

    let mut ids = read(instance_dir, kind)?;
    if ids.iter().any(|existing| existing == id) {
        return Ok(false);
    }
    ids.push(id.to_string());
    write(instance_dir, kind, &ids)?;
    Ok(true)
}

/// Removes an id if present. Returns `false` (no-op) if it wasn't there.
pub fn remove_id(instance_dir: &Path, kind: ListKind, id: &str) -> Result<bool> {
    let mut ids = read(instance_dir, kind)?;
    let original_len = ids.len();
    ids.retain(|existing| existing != id);
    if ids.len() == original_len {
        return Ok(false);
    }
    write(instance_dir, kind, &ids)?;
    Ok(true)
}

/// A SteamID64 is always a 17-digit number starting with the fixed `7656119`
/// prefix every Steam64 account id shares; this rejects pasted SteamID3s,
/// vanity URLs, or other garbage with a clear error instead of writing it to
/// a file Valheim will just silently ignore.
fn validate_steam_id64(id: &str) -> Result<(), String> {
    if id.len() != 17 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "'{id}' is not a valid SteamID64: expected exactly 17 digits"
        ));
    }
    if !id.starts_with("7656119") {
        return Err(format!(
            "'{id}' is not a valid SteamID64: expected it to start with '7656119'"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("odin-lists-test-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const VALID_ID_A: &str = "76561197960287930";
    const VALID_ID_B: &str = "76561197960287931";

    #[test]
    fn missing_file_reads_as_empty() {
        let dir = temp_dir("missing");
        assert_eq!(read(&dir, ListKind::Admin).unwrap(), Vec::<String>::new());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = temp_dir("roundtrip");
        let ids = vec![VALID_ID_A.to_string(), VALID_ID_B.to_string()];
        write(&dir, ListKind::Banned, &ids).unwrap();
        assert_eq!(read(&dir, ListKind::Banned).unwrap(), ids);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_rejects_invalid_id() {
        let dir = temp_dir("invalid");
        let err = write(&dir, ListKind::Admin, &["not-a-steamid".to_string()]).unwrap_err();
        assert!(err.to_string().contains("not a valid SteamID64"));
        assert!(!list_path(&dir, ListKind::Admin).exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_id_is_idempotent() {
        let dir = temp_dir("add-idempotent");
        assert!(add_id(&dir, ListKind::Permitted, VALID_ID_A).unwrap());
        assert!(!add_id(&dir, ListKind::Permitted, VALID_ID_A).unwrap());
        assert_eq!(
            read(&dir, ListKind::Permitted).unwrap(),
            vec![VALID_ID_A.to_string()]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_id_is_idempotent() {
        let dir = temp_dir("remove-idempotent");
        add_id(&dir, ListKind::Admin, VALID_ID_A).unwrap();
        assert!(remove_id(&dir, ListKind::Admin, VALID_ID_A).unwrap());
        assert!(!remove_id(&dir, ListKind::Admin, VALID_ID_A).unwrap());
        assert_eq!(read(&dir, ListKind::Admin).unwrap(), Vec::<String>::new());
        std::fs::remove_dir_all(&dir).ok();
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
