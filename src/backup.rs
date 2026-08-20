use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};

use crate::instance::Instance;
use crate::mods;
use crate::paths;

pub struct BackupEntry {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
}

fn backups_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join("backups")
}

fn backup_id_now() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// Zips the instance's `saves/` directory into `<instance_dir>/backups/<id>.zip`.
pub fn create(instance: &Instance) -> Result<PathBuf> {
    let dir = backups_dir(&instance.dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create backups dir {}", dir.display()))?;

    let id = backup_id_now();
    let backup_path = dir.join(format!("{id}.zip"));
    zip_directory(&paths::instance_saves_dir(&instance.dir), &backup_path)?;
    Ok(backup_path)
}

/// Lists available backups for an instance, newest first.
pub fn list(instance_dir: &Path) -> Result<Vec<BackupEntry>> {
    let dir = backups_dir(instance_dir);
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
        });
    }
    entries.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(entries)
}

/// Restores `<instance_dir>/backups/<backup_id>.zip` over `saves/`, after
/// first snapshotting the current `saves/` (so a restore is never a
/// one-way, unrecoverable overwrite). Caller is responsible for checking
/// the instance isn't running.
pub fn restore(instance: &Instance, backup_id: &str) -> Result<()> {
    let backup_path = backups_dir(&instance.dir).join(format!("{backup_id}.zip"));
    if !backup_path.is_file() {
        bail!(
            "backup '{backup_id}' not found for instance '{}'; run `valheim restore {}` with no id to list available backups",
            instance.state.name,
            instance.state.name
        );
    }

    create(instance).context("failed to snapshot current saves before restoring")?;

    let saves_dir = paths::instance_saves_dir(&instance.dir);
    std::fs::remove_dir_all(&saves_dir).ok();
    std::fs::create_dir_all(&saves_dir)?;
    mods::extract_zip_to_dir(&backup_path, &saves_dir)
        .with_context(|| format!("failed to restore backup '{backup_id}'"))
}

fn zip_directory(source_dir: &Path, dest_zip: &Path) -> Result<()> {
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
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if entry.file_type()?.is_dir() {
            add_dir_to_zip(writer, root, &path, options)?;
        } else {
            writer.start_file(relative, options)?;
            let bytes = std::fs::read(&path)?;
            writer.write_all(&bytes)?;
        }
    }
    Ok(())
}
