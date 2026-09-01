use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::thunderstore::{self, BEPINEX_MOD_ID, ModRef};
use super::{CleanupDir, effective_source_root, extract_zip_to_dir};
use crate::db::Db;

/// Downloads and unpacks the BepInEx pack for Valheim directly into the
/// instance directory (the pack's own layout mirrors the game's install root).
pub fn bootstrap(db: &Db, instance_dir: &Path) -> Result<String> {
    let index = thunderstore::fetch_index(db)?;
    let mod_ref = ModRef::parse(BEPINEX_MOD_ID)?;
    let (_package, version) = thunderstore::resolve(&mod_ref, &index)?;

    let tmp_dir = instance_dir.join(".bepinex-install-tmp");
    std::fs::create_dir_all(&tmp_dir)?;
    let _cleanup = CleanupDir(&tmp_dir);

    let zip_path = thunderstore::download_zip(&version.download_url, &tmp_dir)?;
    let extract_dir = tmp_dir.join("extracted");
    extract_zip_to_dir(&zip_path, &extract_dir)?;

    let source_root = effective_source_root(&extract_dir)?;
    super::copy_dir_contents_excluding_metadata(&source_root, instance_dir)
        .context("failed to install BepInEx pack into instance directory")?;

    Ok(version.version_number.clone())
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    AlreadyCurrent { version: String },
    Updated { from: Option<String>, to: String },
}

pub fn latest_version(db: &Db) -> Result<thunderstore::ThunderstoreVersion> {
    let index = thunderstore::fetch_index(db)?;
    let mod_ref = ModRef::parse(BEPINEX_MOD_ID)?;
    let (_, version) = thunderstore::resolve(&mod_ref, &index)?;
    Ok(version.clone())
}

/// Compares numeric dotted versions without treating lexical ordering as semantic ordering.
pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let parse = |value: &str| {
        value
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let left = parse(left);
    let right = parse(right);
    let length = left.len().max(right.len());
    (0..length)
        .map(|index| {
            left.get(index)
                .copied()
                .unwrap_or(0)
                .cmp(&right.get(index).copied().unwrap_or(0))
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

pub fn update_latest(
    db: &Db,
    instance_name: &str,
    instance_dir: &Path,
    installed_version: Option<&str>,
    log: impl Fn(&str),
) -> Result<UpdateOutcome> {
    log("resolving latest BepInEx version");
    let latest = latest_version(db)?;
    if installed_version
        .is_some_and(|current| compare_versions(current, &latest.version_number) != Ordering::Less)
    {
        log("BepInEx is already up to date; nothing to do");
        return Ok(UpdateOutcome::AlreadyCurrent {
            version: latest.version_number,
        });
    }

    let tmp_dir = instance_dir.join(format!(".bepinex-update-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_dir)?;
    let _cleanup = CleanupDir(&tmp_dir);
    log("downloading BepInEx package");
    let zip_path = thunderstore::download_zip(&latest.download_url, &tmp_dir)?;
    let extract_dir = tmp_dir.join("extracted");
    extract_zip_to_dir(&zip_path, &extract_dir)?;
    let source_root = effective_source_root(&extract_dir)?;
    validate_package(&source_root)?;

    log("installing validated BepInEx package");
    let backup_dir = tmp_dir.join("backup");
    let files = package_files(&source_root)?;
    let mut copied = Vec::new();
    let install_result = (|| -> Result<()> {
        for relative in &files {
            let source = source_root.join(relative);
            let destination = instance_dir.join(relative);
            if destination.exists() {
                let backup = backup_dir.join(relative);
                if let Some(parent) = backup.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&destination, &backup)
                    .with_context(|| format!("failed to back up {}", destination.display()))?;
            }
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source, &destination)
                .with_context(|| format!("failed to install {}", destination.display()))?;
            copied.push(relative.clone());
        }
        Ok(())
    })();

    if let Err(error) = install_result {
        log("installation failed; rolling back overwritten files");
        rollback(instance_dir, &backup_dir, copied)?;
        return Err(error);
    }

    if let Err(error) =
        crate::db::instances::set_bepinex(db, instance_name, true, Some(&latest.version_number))
    {
        log("failed to persist BepInEx version; rolling back installed files");
        rollback(instance_dir, &backup_dir, copied)?;
        return Err(error);
    }
    log("BepInEx update completed");
    Ok(UpdateOutcome::Updated {
        from: installed_version.map(str::to_string),
        to: latest.version_number,
    })
}

fn validate_package(root: &Path) -> Result<()> {
    if !root.join("BepInEx/core/BepInEx.Preloader.dll").is_file() {
        bail!("downloaded BepInEx package is missing BepInEx/core/BepInEx.Preloader.dll");
    }
    Ok(())
}

fn package_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, files)?;
            } else if path.is_file() {
                let relative = path.strip_prefix(root)?.to_path_buf();
                let protected = relative.starts_with("BepInEx/config")
                    || relative.starts_with("BepInEx/plugins");
                let metadata = relative.file_name().is_some_and(|name| {
                    matches!(
                        name.to_str(),
                        Some("icon.png" | "manifest.json" | "README.md")
                    )
                });
                if !protected && !metadata {
                    files.push(relative);
                }
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

fn rollback(instance_dir: &Path, backup_dir: &Path, copied: Vec<PathBuf>) -> Result<()> {
    for relative in copied.into_iter().rev() {
        let destination = instance_dir.join(&relative);
        let backup = backup_dir.join(&relative);
        if backup.exists() {
            std::fs::copy(&backup, &destination).with_context(|| {
                format!(
                    "failed to restore {} during rollback",
                    destination.display()
                )
            })?;
        } else {
            std::fs::remove_file(&destination).ok();
        }
    }
    Ok(())
}

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn numeric_versions_compare_component_by_component() {
        assert_eq!(compare_versions("5.4.2305", "5.4.2202"), Ordering::Greater);
        assert_eq!(compare_versions("5.4.10", "5.4.9"), Ordering::Greater);
        assert_eq!(compare_versions("5.4", "5.4.0"), Ordering::Equal);
    }

    #[test]
    fn package_validation_happens_before_installation_and_protected_dirs_are_excluded() {
        let root =
            std::env::temp_dir().join(format!("odin-bepinex-package-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("BepInEx/core")).unwrap();
        assert!(validate_package(&root).is_err());

        std::fs::write(root.join("BepInEx/core/BepInEx.Preloader.dll"), "core").unwrap();
        std::fs::create_dir_all(root.join("BepInEx/config")).unwrap();
        std::fs::create_dir_all(root.join("BepInEx/plugins/example")).unwrap();
        std::fs::write(root.join("BepInEx/config/local.cfg"), "local").unwrap();
        std::fs::write(root.join("BepInEx/plugins/example/plugin.dll"), "plugin").unwrap();

        validate_package(&root).unwrap();
        let files = package_files(&root).unwrap();
        assert_eq!(
            files,
            vec![PathBuf::from("BepInEx/core/BepInEx.Preloader.dll")]
        );
        std::fs::remove_dir_all(root).ok();
    }
}
