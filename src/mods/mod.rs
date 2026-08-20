pub mod bepinex;
pub mod thunderstore;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::instance::Instance;
use crate::instance::state::InstalledMod;
use crate::paths::{self, Paths};
use thunderstore::ModRef;

/// Package metadata files that live at a Thunderstore package's zip root
/// alongside the actual mod payload; these aren't part of the mod itself.
const METADATA_ENTRIES: &[&str] = &["icon.png", "manifest.json", "README.md", "CHANGELOG.md"];

pub fn add(paths: &Paths, server_name: &str, mod_id: &str) -> Result<()> {
    let mut instance = Instance::load_existing(paths, server_name)?;

    if !instance.state.bepinex_installed {
        tracing::info!(
            instance = server_name,
            "BepInEx not installed yet; bootstrapping"
        );
        bepinex::bootstrap(paths, &instance.dir)?;
        instance.state.bepinex_installed = true;
        instance.save()?;
    }

    let mod_ref = ModRef::parse(mod_id)?;
    let index = thunderstore::fetch_index(paths)?;
    let (_package, version) = thunderstore::resolve(&mod_ref, &index)?;

    install_plugin(&instance.dir, &mod_ref, &version.download_url)?;

    let entry = InstalledMod {
        mod_id: mod_ref.mod_id(),
        version: version.version_number.clone(),
        installed_at: Utc::now(),
    };
    instance
        .state
        .installed_mods
        .retain(|m| m.mod_id != entry.mod_id);
    instance.state.installed_mods.push(entry);
    instance.save()?;

    if crate::instance::lifecycle::is_running(&instance)? {
        tracing::warn!(
            instance = server_name,
            "instance is currently running; the new mod won't be loaded until it's restarted"
        );
    }

    Ok(())
}

pub fn update(paths: &Paths, server_name: &str) -> Result<()> {
    let mut instance = Instance::load_existing(paths, server_name)?;
    let index = thunderstore::fetch_index(paths)?;

    let mut any_updated = false;
    for installed in instance.state.installed_mods.clone() {
        let mod_ref = ModRef::parse(&installed.mod_id)?;
        let (_package, latest) = thunderstore::resolve(&mod_ref, &index)?;
        if latest.version_number == installed.version {
            continue;
        }

        tracing::info!(
            instance = server_name,
            mod_id = installed.mod_id,
            from = installed.version,
            to = latest.version_number,
            "updating mod"
        );
        install_plugin(&instance.dir, &mod_ref, &latest.download_url)?;

        if let Some(entry) = instance
            .state
            .installed_mods
            .iter_mut()
            .find(|m| m.mod_id == installed.mod_id)
        {
            entry.version = latest.version_number.clone();
            entry.installed_at = Utc::now();
        }
        any_updated = true;
    }

    instance.save()?;

    if !any_updated {
        println!("all mods for '{server_name}' are already up to date");
    }

    if crate::instance::lifecycle::is_running(&instance)? && any_updated {
        tracing::warn!(
            instance = server_name,
            "instance is currently running; updated mods won't take effect until it's restarted"
        );
    }

    Ok(())
}

fn install_plugin(instance_dir: &Path, mod_ref: &ModRef, download_url: &str) -> Result<()> {
    let tmp_dir = instance_dir.join(format!(".mod-install-tmp-{}", mod_ref.mod_id()));
    std::fs::create_dir_all(&tmp_dir)?;

    let zip_path = thunderstore::download_zip(download_url, &tmp_dir)?;
    let extract_dir = tmp_dir.join("extracted");
    extract_zip_to_dir(&zip_path, &extract_dir)?;

    let plugin_dir = paths::instance_bepinex_dir(instance_dir)
        .join("plugins")
        .join(mod_ref.mod_id());
    if plugin_dir.exists() {
        std::fs::remove_dir_all(&plugin_dir)?;
    }
    std::fs::create_dir_all(&plugin_dir)?;

    copy_dir_contents_excluding_metadata(&extract_dir, &plugin_dir)
        .with_context(|| format!("failed to install mod '{}'", mod_ref.mod_id()))?;

    std::fs::remove_dir_all(&tmp_dir).ok();
    Ok(())
}

pub fn extract_zip_to_dir(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir)?;
    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("failed to open zip {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip {}", zip_path.display()))?;
    archive
        .extract(dest_dir)
        .with_context(|| format!("failed to extract zip {}", zip_path.display()))
}

/// If `dir` contains exactly one entry and it's a directory, returns that
/// subdirectory (unwrapping a package's top-level wrapper folder). Otherwise
/// returns `dir` itself.
pub fn flatten_single_root_dir(dir: &Path) -> Result<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<std::result::Result<_, _>>()?;
    if entries.len() == 1 && entries[0].file_type()?.is_dir() {
        return Ok(entries.remove(0).path());
    }
    Ok(dir.to_path_buf())
}

/// Copies everything under `source` into `dest`, skipping Thunderstore package
/// metadata files that live at the root (icon.png, manifest.json, etc).
pub fn copy_dir_contents_excluding_metadata(source: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_name = entry.file_name();
        if METADATA_ENTRIES
            .iter()
            .any(|m| file_name.to_string_lossy() == *m)
        {
            continue;
        }
        let target = dest.join(&file_name);
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn copy_dir_all(source: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
