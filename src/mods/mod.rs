//! Manages mods installed from Thunderstore.
//!
//! Downloaded mod payloads live in one global store (`paths.mod_dir(mod_id)`),
//! one copy per mod id shared across every instance. An instance "has" a mod
//! by symlinking `BepInEx/plugins/<mod_id>` into that shared copy
//! (`link_into_instance`); disabling a mod just removes that symlink
//! (`remove_link_if_present`) without touching the shared download. Each
//! instance's own state (`InstalledMod`) only records which version it last
//! saw and whether it's currently linked in.

pub mod bepinex;
pub mod thunderstore;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

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

    let global_dir = ensure_global_mod(
        paths,
        &mod_ref,
        &version.version_number,
        &version.download_url,
    )?;
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

    // Mod ids are collected up front (rather than cloning the whole
    // `Vec<InstalledMod>`) so the loop can still mutate `installed_mods` by
    // id lookup without holding a borrow across the loop body.
    let mod_ids: Vec<String> = instance
        .state
        .installed_mods
        .iter()
        .map(|m| m.mod_id.clone())
        .collect();

    let mut any_updated = false;
    for mod_id in mod_ids {
        let installed_idx = instance
            .state
            .installed_mods
            .iter()
            .position(|m| m.mod_id == mod_id)
            .expect("mod_id came from installed_mods and is never removed mid-loop");
        let installed = &instance.state.installed_mods[installed_idx];

        let mod_ref = ModRef::parse(&mod_id)?;
        let (_package, latest) = thunderstore::resolve(&mod_ref, &index)?;
        if latest.version_number == installed.version {
            continue;
        }

        tracing::info!(
            instance = server_name,
            mod_id = mod_id,
            from = installed.version,
            to = latest.version_number,
            "updating mod"
        );
        // A disabled mod stays disabled: just record the new version so a
        // later `enable` links to it; don't touch `plugins/<mod_id>`.
        let enabled = installed.enabled;

        let global_dir = ensure_global_mod(
            paths,
            &mod_ref,
            &latest.version_number,
            &latest.download_url,
        )?;
        if enabled {
            link_into_instance(&instance.dir, &mod_id, &global_dir)?;
        }

        let entry = &mut instance.state.installed_mods[installed_idx];
        entry.version.clone_from(&latest.version_number);
        entry.installed_at = Utc::now();
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

#[derive(Debug, Clone, Serialize)]
pub struct GlobalModInstanceEntry {
    pub instance: String,
    pub version: String,
    pub enabled: bool,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlobalMod {
    pub mod_id: String,
    /// Version currently held in the shared store, if it's still there
    /// (`None` for a mod that some instance's state still references but
    /// whose download was manually removed from `paths.mods_dir()`).
    pub global_version: Option<String>,
    pub instances: Vec<GlobalModInstanceEntry>,
}

/// Aggregates the shared mod store (`paths.mods_dir()`) with what each
/// instance currently has installed, for a cross-instance view. A mod
/// present in the store but installed on zero instances is still included —
/// it's an orphaned download taking up disk space.
pub fn list_global(paths: &Paths) -> Result<Vec<GlobalMod>> {
    let mut mods: BTreeMap<String, GlobalMod> = BTreeMap::new();

    let mods_dir = paths.mods_dir();
    if mods_dir.is_dir() {
        for entry in std::fs::read_dir(&mods_dir)
            .with_context(|| format!("failed to read {}", mods_dir.display()))?
        {
            let entry = entry?;
            let mod_id = entry.file_name().to_string_lossy().into_owned();
            // Skip the staging dirs `ensure_global_mod` uses while installing.
            if mod_id.starts_with('.') || !entry.file_type()?.is_dir() {
                continue;
            }
            mods.entry(mod_id.clone())
                .or_insert_with(|| GlobalMod {
                    mod_id: mod_id.clone(),
                    global_version: None,
                    instances: Vec::new(),
                })
                .global_version = current_marker_version(&entry.path());
        }
    }

    for instance in crate::instance::list_all(paths)? {
        let running = crate::instance::lifecycle::is_running(&instance)?;
        for installed in &instance.state.installed_mods {
            mods.entry(installed.mod_id.clone())
                .or_insert_with(|| GlobalMod {
                    mod_id: installed.mod_id.clone(),
                    global_version: None,
                    instances: Vec::new(),
                })
                .instances
                .push(GlobalModInstanceEntry {
                    instance: instance.state.name.clone(),
                    version: installed.version.clone(),
                    enabled: installed.enabled,
                    running,
                });
        }
    }

    Ok(mods.into_values().collect())
}

/// Removes a mod's payload from the global store. Refuses if any instance
/// still references it — it must be removed per-instance first via `remove`.
pub fn prune_global(paths: &Paths, mod_id: &str) -> Result<()> {
    let in_use = crate::instance::list_all(paths)?
        .into_iter()
        .any(|instance| {
            instance
                .state
                .installed_mods
                .iter()
                .any(|m| m.mod_id == mod_id)
        });
    if in_use {
        anyhow::bail!(
            "mod '{mod_id}' is still installed on at least one instance; remove it there first"
        );
    }

    let dir = paths.mod_dir(mod_id);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("failed to remove {}", dir.display()))?;
    }
    Ok(())
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

    // Suffixed with a fresh uuid (not just the mod_id) so two concurrent
    // installs of the same mod for different instances — e.g. installing to
    // several instances at once from the dashboard — don't share, and race
    // on cleaning up, the same staging directory.
    let staging_dir = paths.mods_dir().join(format!(
        ".install-tmp-{}-{}",
        mod_ref.mod_id(),
        uuid::Uuid::new_v4()
    ));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("odin-test-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn remove_link_if_present_is_noop_when_nothing_exists() {
        let dir = temp_dir("noop");
        remove_link_if_present(&dir.join("missing")).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_link_if_present_removes_symlink_without_deleting_target() {
        let dir = temp_dir("symlink");
        let target = dir.join("target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("marker"), b"keep me").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        remove_link_if_present(&link).unwrap();

        assert!(!link.exists());
        assert!(target.join("marker").is_file());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_link_if_present_removes_dangling_symlink() {
        let dir = temp_dir("dangling");
        let link = dir.join("link");
        std::os::unix::fs::symlink(dir.join("gone"), &link).unwrap();
        // A dangling symlink reports false for both is_dir() and is_file(), so
        // this exercises the symlink_metadata branch rather than the plain-dir one.
        assert!(!link.is_dir());

        remove_link_if_present(&link).unwrap();

        assert!(std::fs::symlink_metadata(&link).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_link_if_present_removes_real_directory() {
        let dir = temp_dir("realdir");
        let path = dir.join("plain");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("file"), b"data").unwrap();

        remove_link_if_present(&path).unwrap();

        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn link_into_instance_replaces_existing_link() {
        let dir = temp_dir("relink");
        let instance_dir = dir.join("instance");
        let global_a = dir.join("global-a");
        let global_b = dir.join("global-b");
        std::fs::create_dir_all(&global_a).unwrap();
        std::fs::create_dir_all(&global_b).unwrap();

        link_into_instance(&instance_dir, "owner-mod", &global_a).unwrap();
        link_into_instance(&instance_dir, "owner-mod", &global_b).unwrap();

        let link = active_plugin_dir(&instance_dir, "owner-mod");
        assert_eq!(std::fs::read_link(&link).unwrap(), global_b);
        std::fs::remove_dir_all(&dir).ok();
    }
}
