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

    let global_dir = ensure_global_mod(paths, &mod_ref, &version.version_number, &version.download_url)?;
    link_into_instance(&instance.dir, &mod_ref.mod_id(), &global_dir)?;

    let entry = InstalledMod {
        mod_id: mod_ref.mod_id(),
        version: version.version_number.clone(),
        installed_at: Utc::now(),
        enabled: true,
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
        let global_dir =
            ensure_global_mod(paths, &mod_ref, &latest.version_number, &latest.download_url)?;
        // A disabled mod stays disabled: just record the new version so a
        // later `enable` links to it; don't touch `plugins/<mod_id>`.
        if installed.enabled {
            link_into_instance(&instance.dir, &installed.mod_id, &global_dir)?;
        }

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

/// Reads an instance's installed mods from its state — no network call.
pub fn list(paths: &Paths, server_name: &str) -> Result<Vec<InstalledMod>> {
    let instance = Instance::load_existing(paths, server_name)?;
    Ok(instance.state.installed_mods)
}

pub fn remove(paths: &Paths, server_name: &str, mod_id: &str) -> Result<()> {
    let mut instance = Instance::load_existing(paths, server_name)?;

    if !instance
        .state
        .installed_mods
        .iter()
        .any(|m| m.mod_id == mod_id)
    {
        anyhow::bail!("mod '{mod_id}' is not installed on '{server_name}'");
    }

    // Only the instance's symlink is removed; the global version dir (which
    // may be shared by other instances/mods) is deliberately left in place.
    remove_link_if_present(&active_plugin_dir(&instance.dir, mod_id))?;

    instance.state.installed_mods.retain(|m| m.mod_id != mod_id);
    instance.save()?;

    if crate::instance::lifecycle::is_running(&instance)? {
        tracing::warn!(
            instance = server_name,
            "instance is currently running; the mod stays loaded until it's restarted"
        );
    }

    Ok(())
}

/// Symlinks or unlinks a mod's entry under `plugins/` and flips its stored
/// `enabled` flag. Returns whether anything actually changed (false if the
/// mod was already in the requested state).
pub fn set_enabled(paths: &Paths, server_name: &str, mod_id: &str, enabled: bool) -> Result<bool> {
    let mut instance = Instance::load_existing(paths, server_name)?;

    let current = instance
        .state
        .installed_mods
        .iter()
        .find(|m| m.mod_id == mod_id)
        .map(|m| m.enabled)
        .ok_or_else(|| anyhow::anyhow!("mod '{mod_id}' is not installed on '{server_name}'"))?;

    if current == enabled {
        return Ok(false);
    }

    if enabled {
        let global_dir = paths.mod_dir(mod_id);
        if !global_dir.is_dir() {
            anyhow::bail!(
                "'{mod_id}' is missing from the global mod store ({}); \
                 run `odin mods update {server_name}` or re-add the mod",
                global_dir.display()
            );
        }
        link_into_instance(&instance.dir, mod_id, &global_dir)?;
    } else {
        remove_link_if_present(&active_plugin_dir(&instance.dir, mod_id))?;
    }

    if let Some(entry) = instance
        .state
        .installed_mods
        .iter_mut()
        .find(|m| m.mod_id == mod_id)
    {
        entry.enabled = enabled;
    }
    instance.save()?;

    if crate::instance::lifecycle::is_running(&instance)? {
        tracing::warn!(
            instance = server_name,
            "instance is currently running; the change won't take effect until it's restarted"
        );
    }

    Ok(true)
}

fn active_plugin_dir(instance_dir: &Path, mod_id: &str) -> PathBuf {
    paths::instance_bepinex_dir(instance_dir)
        .join("plugins")
        .join(mod_id)
}

/// Marker file dropped alongside a mod's payload in the global store,
/// recording which version is currently there so `ensure_global_mod` can
/// skip re-downloading when it's already the one asked for.
const VERSION_MARKER: &str = ".odin-version";

/// Ensures `paths.mod_dir(mod_id)` holds `version_number`, downloading and
/// replacing its contents if it currently holds a different (or no)
/// version. There's one shared copy per mod_id (not per version), so
/// updating it affects every instance currently symlinking it.
fn ensure_global_mod(
    paths: &Paths,
    mod_ref: &ModRef,
    version_number: &str,
    download_url: &str,
) -> Result<PathBuf> {
    let final_dir = paths.mod_dir(&mod_ref.mod_id());
    if current_marker_version(&final_dir).as_deref() == Some(version_number) {
        return Ok(final_dir);
    }

    let staging_dir = paths
        .mods_dir()
        .join(format!(".install-tmp-{}", mod_ref.mod_id()));
    std::fs::create_dir_all(&staging_dir)?;

    let zip_path = thunderstore::download_zip(download_url, &staging_dir)?;
    let extract_dir = staging_dir.join("extracted");
    extract_zip_to_dir(&zip_path, &extract_dir)?;
    let source_root = effective_source_root(&extract_dir)?;

    if final_dir.is_dir() {
        std::fs::remove_dir_all(&final_dir)
            .with_context(|| format!("failed to remove stale copy at {}", final_dir.display()))?;
    }
    if let Some(parent) = final_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    copy_dir_contents_excluding_metadata(&source_root, &final_dir)
        .with_context(|| format!("failed to install mod '{}'", mod_ref.mod_id()))?;
    std::fs::write(final_dir.join(VERSION_MARKER), version_number)
        .with_context(|| format!("failed to record version for '{}'", mod_ref.mod_id()))?;

    std::fs::remove_dir_all(&staging_dir).ok();
    Ok(final_dir)
}

fn current_marker_version(mod_dir: &Path) -> Option<String> {
    std::fs::read_to_string(mod_dir.join(VERSION_MARKER))
        .ok()
        .map(|s| s.trim().to_string())
}

/// Removes whatever is at `path` (symlink or real directory) without
/// following a symlink into its target. No-op if nothing is there — this
/// also correctly clears a *dangling* symlink, which a plain `is_dir()`
/// check would miss (it returns false for a symlink whose target is gone)
/// and so would leave behind forever.
fn remove_link_if_present(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(path)
            .with_context(|| format!("failed to remove symlink {}", path.display())),
        Ok(_) => std::fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("failed to stat {}", path.display())),
    }
}

/// Symlinks `global_version_dir` into `<instance_dir>/BepInEx/plugins/<mod_id>`,
/// replacing whatever was there before.
fn link_into_instance(instance_dir: &Path, mod_id: &str, global_version_dir: &Path) -> Result<()> {
    let link_path = active_plugin_dir(instance_dir, mod_id);
    remove_link_if_present(&link_path)?;
    if let Some(parent) = link_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(global_version_dir, &link_path).with_context(|| {
        format!(
            "failed to symlink {} -> {}",
            link_path.display(),
            global_version_dir.display()
        )
    })
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

/// Ignoring known Thunderstore metadata files, if exactly one entry remains
/// and it's a directory, returns that subdirectory (unwrapping a package's
/// top-level wrapper folder, e.g. `BepInExPack_Valheim/`). Otherwise returns
/// `dir` itself.
pub fn effective_source_root(dir: &Path) -> Result<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir)?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|e| {
            !METADATA_ENTRIES
                .iter()
                .any(|m| e.file_name().to_string_lossy() == *m)
        })
        .collect();
    if entries.len() == 1 && entries[0].file_type()?.is_dir() {
        return Ok(entries[0].path());
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
