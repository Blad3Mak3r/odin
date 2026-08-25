//! BepInEx's per-plugin config files: plain `.cfg` text files that BepInEx
//! itself generates under `<instance_dir>/BepInEx/config/` the first time a
//! plugin loads. Unlike the shared/symlinked mod store, these are genuinely
//! per-instance and named after the plugin's GUID rather than its
//! Thunderstore `mod_id`, so callers discover them via `list` rather than
//! deriving a filename from an installed mod.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use thiserror::Error;

use crate::paths;

/// Validation failures a caller (e.g. the web API) may want to distinguish
/// from other, unexpected errors — these always mean "the input was bad",
/// never "something went wrong on our end".
#[derive(Debug, Error)]
pub enum ConfigFileError {
    #[error("'{0}' is not a valid BepInEx config filename")]
    InvalidFilename(String),
    #[error("config file '{0}' does not exist")]
    NotFound(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFileEntry {
    pub filename: String,
    pub size_bytes: u64,
}

/// Lists `.cfg` files directly present in the instance's BepInEx config
/// directory. A missing directory (BepInEx never bootstrapped, or no plugin
/// has generated a config yet) reads as an empty list, not an error.
pub fn list(instance_dir: &Path) -> Result<Vec<ConfigFileEntry>> {
    let dir = paths::instance_bepinex_config_dir(instance_dir);
    let mut out = Vec::new();

    let read_dir = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", dir.display())),
    };

    for entry in read_dir {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !name.ends_with(".cfg") || !entry.file_type()?.is_file() {
            continue;
        }
        let size_bytes = entry.metadata()?.len();
        out.push(ConfigFileEntry {
            filename: name,
            size_bytes,
        });
    }

    out.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(out)
}

/// Reads a config file's contents. `filename` is client-controlled (it's a
/// URL path segment on the web API), so it's validated syntactically and
/// then re-checked against what's actually present on disk before anything
/// is touched — see `resolve_existing_path`.
pub fn read(instance_dir: &Path, filename: &str) -> Result<String> {
    let path = resolve_existing_path(instance_dir, filename)?;
    std::fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

/// Overwrites an existing config file's contents. Refuses to create a new
/// file — BepInEx config files are only ever edited here, never authored
/// from scratch by the dashboard. Written atomically (temp file + rename),
/// mirroring `instance::lists::write`.
pub fn write(instance_dir: &Path, filename: &str, content: &str) -> Result<()> {
    let path = resolve_existing_path(instance_dir, filename)?;

    let tmp_path = path.with_extension("cfg.tmp");
    std::fs::write(&tmp_path, content)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, &path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            tmp_path.display(),
            path.display()
        )
    })
}

/// Syntactic pre-check on a client-supplied filename, before any filesystem
/// access: non-empty, no path separators, no `..`, no leading dot, and must
/// end in `.cfg` (the only extension BepInEx ever writes there).
fn validate_filename(filename: &str) -> Result<(), ConfigFileError> {
    let valid = !filename.is_empty()
        && !filename.contains('/')
        && !filename.contains('\\')
        && !filename.contains("..")
        && !filename.starts_with('.')
        && filename.ends_with(".cfg");
    if !valid {
        return Err(ConfigFileError::InvalidFilename(filename.to_string()));
    }
    Ok(())
}

/// Validates `filename` and resolves it to a path that is confirmed to be an
/// existing file inside the instance's BepInEx config directory. This is the
/// single choke point `read` and `write` both go through, so a client can
/// never touch anything `list` wouldn't have shown:
///
/// 1. `validate_filename` rejects anything shaped like a path (separators,
///    `..`, non-`.cfg`) before any I/O happens.
/// 2. Joining onto the config dir and checking `is_file()` re-validates the
///    filename against what's actually on disk, not just a plausible-looking
///    string.
/// 3. Canonicalizing both paths and checking containment is a final
///    backstop in case anything upstream ever changes (e.g. a plugin
///    symlinking its own config elsewhere) — pure defense in depth on top
///    of 1–2, which already make escape effectively impossible.
fn resolve_existing_path(instance_dir: &Path, filename: &str) -> Result<PathBuf> {
    validate_filename(filename)?;

    let dir = paths::instance_bepinex_config_dir(instance_dir);
    let path = dir.join(filename);
    if !path.is_file() {
        return Err(ConfigFileError::NotFound(filename.to_string()).into());
    }

    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
    let canonical_dir = dir
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", dir.display()))?;
    if !canonical_path.starts_with(&canonical_dir) {
        return Err(ConfigFileError::InvalidFilename(filename.to_string()).into());
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "odin-mods-config-test-{label}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config_dir(instance_dir: &Path) -> PathBuf {
        let dir = paths::instance_bepinex_config_dir(instance_dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_config_dir_lists_as_empty() {
        let dir = temp_dir("missing-dir");
        assert_eq!(list(&dir).unwrap().len(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_only_returns_cfg_files() {
        let dir = temp_dir("list-filter");
        let cfg_dir = config_dir(&dir);
        std::fs::write(cfg_dir.join("Plugin.GUID.cfg"), "[Section]\nkey = value\n").unwrap();
        std::fs::write(cfg_dir.join("notes.json"), "{}").unwrap();
        std::fs::write(cfg_dir.join(".hidden.cfg"), "secret").unwrap();

        let entries = list(&dir).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "Plugin.GUID.cfg");
        assert!(entries[0].size_bytes > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_then_write_round_trips() {
        let dir = temp_dir("roundtrip");
        let cfg_dir = config_dir(&dir);
        std::fs::write(cfg_dir.join("Plugin.cfg"), "[Section]\nkey = old\n").unwrap();

        write(&dir, "Plugin.cfg", "[Section]\nkey = new\n").unwrap();
        assert_eq!(read(&dir, "Plugin.cfg").unwrap(), "[Section]\nkey = new\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_rejects_unknown_filename() {
        let dir = temp_dir("unknown-filename");
        config_dir(&dir);
        let err = write(&dir, "DoesNotExist.cfg", "content").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_rejects_path_traversal() {
        let dir = temp_dir("traversal");
        config_dir(&dir);
        for bad in ["../../etc/passwd", "../outside.cfg", "sub/dir.cfg"] {
            let err = write(&dir, bad, "content").unwrap_err();
            assert!(
                err.to_string()
                    .contains("not a valid BepInEx config filename"),
                "expected InvalidFilename for {bad:?}, got: {err}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_rejects_non_cfg_extension() {
        let dir = temp_dir("non-cfg");
        let cfg_dir = config_dir(&dir);
        std::fs::write(cfg_dir.join("Plugin.txt"), "content").unwrap();
        let err = write(&dir, "Plugin.txt", "new content").unwrap_err();
        assert!(
            err.to_string()
                .contains("not a valid BepInEx config filename")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_missing_file_is_not_found_error() {
        let dir = temp_dir("read-missing");
        config_dir(&dir);
        let err = read(&dir, "Missing.cfg").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
