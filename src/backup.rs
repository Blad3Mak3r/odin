use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;

use crate::backup_storage::{self, RemoteObject, StorageProvider};
use crate::db::Db;
use crate::instance::{Instance, InstanceError, lifecycle};
use crate::mods;
use crate::paths;

/// Failures a caller (e.g. the web API) may want to distinguish from other,
/// unexpected errors.
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("backup '{0}' not found")]
    NotFound(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupEntry {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
    pub storage: BackupStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStorage {
    Local,
    AwsS3,
    CloudflareR2,
}

impl From<StorageProvider> for BackupStorage {
    fn from(provider: StorageProvider) -> Self {
        match provider {
            StorageProvider::AwsS3 => Self::AwsS3,
            StorageProvider::CloudflareR2 => Self::CloudflareR2,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BackupRecord {
    pub entry: BackupEntry,
    pub remote: Option<RemoteObject>,
}

fn backup_id_now() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Zips the instance's `saves/` directory into `<instance_dir>/backups/<id>.zip`
/// and records its metadata in the database. When remote storage is enabled,
/// uploads the archive and removes the local file only after the upload and
/// remote metadata have both been persisted successfully.
pub fn create(instance: &Instance, db: &Db) -> Result<BackupEntry> {
    let dir = paths::instance_backups_dir(&instance.dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create backups dir {}", dir.display()))?;

    let id = backup_id_now();
    let backup_path = dir.join(format!("{id}.zip"));
    zip_directory(&paths::instance_saves_dir(&instance.dir), &backup_path)?;

    let size_bytes = std::fs::metadata(&backup_path)?.len();
    let mut entry = BackupEntry {
        id,
        created_at: Utc::now(),
        size_bytes,
        storage: BackupStorage::Local,
    };
    crate::db::backups::insert(db, &instance.state.name, &entry)?;

    if let Some(config) = crate::db::backup_storage::get(db, &instance.state.name)?
        && config.enabled
    {
        let object = config.object_for(&instance.state.name, &entry.id);
        backup_storage::upload(&config, &object, &backup_path)?;
        crate::db::backups::mark_remote(db, &instance.state.name, &entry.id, &object)?;
        std::fs::remove_file(&backup_path).with_context(|| {
            format!(
                "backup was uploaded but the local file could not be removed: {}",
                backup_path.display()
            )
        })?;
        entry.storage = object.provider.into();
    }

    Ok(entry)
}

/// Lists an instance's backups, newest first.
pub fn list(db: &Db, instance_name: &str) -> Result<Vec<BackupEntry>> {
    crate::db::backups::list(db, instance_name)
}

/// Scans `<instance_dir>/backups/*.zip` directly, deriving metadata from
/// each file's mtime/size rather than the database — used only by the
/// bootstrap importer to seed the database from an existing installation.
pub(crate) fn list_from_disk(instance_dir: &Path) -> Result<Vec<BackupEntry>> {
    let dir = paths::instance_backups_dir(instance_dir);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in
        std::fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("zip") {
            continue;
        }
        let id = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let metadata = entry.metadata()?;
        let created_at: DateTime<Utc> = metadata.modified()?.into();
        entries.push(BackupEntry {
            id,
            created_at,
            size_bytes: metadata.len(),
            storage: BackupStorage::Local,
        });
    }
    entries.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(entries)
}

/// Restores `<instance_dir>/backups/<backup_id>.zip` over `saves/`, after
/// first snapshotting the current `saves/` (so a restore is never a
/// one-way, unrecoverable overwrite). Refuses to run while the instance is
/// running (checked here, not just by callers, so every entry point —
/// CLI and web — gets the guard for free).
pub fn restore(instance: &Instance, db: &Db, backup_id: &str) -> Result<()> {
    if lifecycle::is_running(instance)? {
        return Err(InstanceError::AlreadyRunning(instance.state.name.clone()).into());
    }

    let record = crate::db::backups::get(db, &instance.state.name, backup_id)?
        .ok_or_else(|| BackupError::NotFound(backup_id.to_string()))?;
    let backups_dir = paths::instance_backups_dir(&instance.dir);
    let local_path = backups_dir.join(format!("{backup_id}.zip"));
    let (backup_path, remove_after) = match record.remote {
        None => {
            if !local_path.is_file() {
                return Err(BackupError::NotFound(backup_id.to_string()).into());
            }
            (local_path, false)
        }
        Some(object) => {
            std::fs::create_dir_all(&backups_dir)?;
            let temp_path =
                backups_dir.join(format!(".restore-{backup_id}-{}.zip", uuid::Uuid::new_v4()));
            let config = crate::db::backup_storage::get(db, &instance.state.name)?
                .context("backup storage credentials are no longer configured")?;
            if let Err(error) = backup_storage::download(&config, &object, &temp_path) {
                std::fs::remove_file(&temp_path).ok();
                return Err(error);
            }
            (temp_path, true)
        }
    };

    let result = (|| {
        create(instance, db).context("failed to snapshot current saves before restoring")?;
        let saves_dir = paths::instance_saves_dir(&instance.dir);
        std::fs::remove_dir_all(&saves_dir).ok();
        std::fs::create_dir_all(&saves_dir)?;
        mods::extract_zip_to_dir(&backup_path, &saves_dir)
            .with_context(|| format!("failed to restore backup '{backup_id}'"))
    })();
    if remove_after {
        std::fs::remove_file(&backup_path).ok();
    }
    result
}

/// Deletes a backup's zip file and its database row.
pub fn delete(instance: &Instance, db: &Db, backup_id: &str) -> Result<()> {
    let record = crate::db::backups::get(db, &instance.state.name, backup_id)?
        .ok_or_else(|| BackupError::NotFound(backup_id.to_string()))?;
    if let Some(object) = record.remote {
        let config = crate::db::backup_storage::get(db, &instance.state.name)?
            .context("backup storage credentials are no longer configured")?;
        backup_storage::delete(&config, &object)?;
    } else {
        let backup_path =
            paths::instance_backups_dir(&instance.dir).join(format!("{backup_id}.zip"));
        if !backup_path.is_file() {
            return Err(BackupError::NotFound(backup_id.to_string()).into());
        }
        std::fs::remove_file(&backup_path)
            .with_context(|| format!("failed to remove {}", backup_path.display()))?;
    }
    crate::db::backups::delete(db, &instance.state.name, backup_id)
}

pub(crate) fn zip_directory(source_dir: &Path, dest_zip: &Path) -> Result<()> {
    std::fs::create_dir_all(source_dir).ok();
    let file = std::fs::File::create(dest_zip)
        .with_context(|| format!("failed to create {}", dest_zip.display()))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    add_dir_to_zip(&mut writer, source_dir, source_dir, options)?;
    writer.finish().context("failed to finalize zip archive")?;
    Ok(())
}

fn add_dir_to_zip(
    writer: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .expect("path is produced by walking root, so it is always prefixed by it")
            .to_string_lossy()
            .replace('\\', "/");
        if entry.file_type()?.is_dir() {
            add_dir_to_zip(writer, root, &path, options)?;
        } else {
            writer.start_file(relative, options)?;
            let mut file = std::fs::File::open(&path)?;
            std::io::copy(&mut file, writer)?;
        }
    }
    Ok(())
}
