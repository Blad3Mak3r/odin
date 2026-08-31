//! Manages mods installed from Thunderstore.
//!
//! Downloaded payloads live in a versioned global store, one immutable copy
//! per `(mod_id, version)`. Each instance symlinks its enabled mods to the
//! exact version recorded in [`InstalledMod`], so updates are isolated while
//! duplicate downloads remain deduplicated.

pub mod bepinex;
pub mod config;
pub mod nexus;
pub mod source;
pub mod thunderstore;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

use crate::db::Db;
use crate::instance::state::InstalledMod;
use crate::instance::{Instance, InstanceError, lifecycle};
use crate::paths::{self, Paths};
use source::ModSource;
use thunderstore::ModRef;

/// Package metadata files that live at a Thunderstore package's zip root
/// alongside the actual mod payload; these aren't part of the mod itself.
const METADATA_ENTRIES: &[&str] = &["icon.png", "manifest.json", "README.md", "CHANGELOG.md"];
const VERSIONED_STORE_MARKER: &str = ".odin-versioned";

/// Converts the pre-v0.9 mutable `<mods>/<mod_id>` payloads into versioned
/// directories. The copy-first order keeps existing instance symlinks valid
/// until their replacements are ready, and the marker makes retries after a
/// partially completed upgrade idempotent.
pub(crate) fn migrate_legacy_store(paths: &Paths, db: &Db) -> Result<()> {
    let mut versions = BTreeMap::new();
    for (mod_id, version) in crate::db::global_mods::list_all(db)? {
        versions.entry(mod_id).or_insert(version);
    }
    for instance in crate::instance::list_all(paths, db)? {
        for installed in instance.state.installed_mods {
            versions
                .entry(installed.mod_id)
                .or_insert(installed.version);
        }
    }
    for (mod_id, version) in versions {
        migrate_legacy_mod(paths, db, &mod_id, &version)?;
    }
    Ok(())
}

fn migrate_legacy_mod(paths: &Paths, db: &Db, mod_id: &str, version: &str) -> Result<()> {
    validate_version(version)?;
    let root = paths.mod_dir(mod_id);
    if !root.is_dir() || root.join(VERSIONED_STORE_MARKER).is_file() {
        return Ok(());
    }

    let version_dir = paths.mod_version_dir(mod_id, version);
    let legacy_entries = std::fs::read_dir(&root)
        .with_context(|| format!("failed to read legacy mod store {}", root.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|entry| {
            entry.file_name() != VERSIONED_STORE_MARKER
                && entry.file_name() != ".odin-version"
                && entry.path() != version_dir
        })
        .collect::<Vec<_>>();

    if !version_dir.is_dir() {
        let staging = paths
            .mods_dir()
            .join(format!(".migrate-tmp-{mod_id}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&staging)?;
        let _cleanup = CleanupDir(&staging);
        for entry in &legacy_entries {
            let target = staging.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir_all(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), target)?;
            }
        }
        std::fs::rename(&staging, &version_dir).with_context(|| {
            format!("failed to move legacy mod '{mod_id}' into version '{version}'")
        })?;
    }
    crate::db::global_mods::insert(db, mod_id, version)?;

    for mut instance in crate::instance::list_all(paths, db)? {
        let Some(installed) = instance
            .state
            .installed_mods
            .iter_mut()
            .find(|installed| installed.mod_id == mod_id)
        else {
            continue;
        };
        installed.version = version.to_string();
        if installed.enabled {
            link_into_instance(&instance.dir, mod_id, &version_dir)?;
        }
        instance.save(db)?;
    }

    for entry in legacy_entries {
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    let legacy_marker = root.join(".odin-version");
    if legacy_marker.exists() {
        std::fs::remove_file(legacy_marker)?;
    }
    std::fs::write(root.join(VERSIONED_STORE_MARKER), [])?;
    tracing::info!(mod_id, version, "migrated mod payload to versioned store");
    Ok(())
}

fn validate_version(version: &str) -> Result<()> {
    let mut components = Path::new(version).components();
    if version.is_empty()
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        anyhow::bail!("'{version}' is not a safe mod version");
    }
    Ok(())
}

pub fn add(paths: &Paths, db: &Db, server_name: &str, mod_id: &str) -> Result<()> {
    let mut instance = Instance::load_existing(paths, db, server_name)?;
    if lifecycle::is_running(&instance)? {
        return Err(InstanceError::ModsLocked(server_name.to_string()).into());
    }
    ensure_bepinex(db, &mut instance)?;

    let (canonical_mod_id, version, download_url) = match source::mod_source(mod_id) {
        ModSource::Thunderstore => {
            let mod_ref = ModRef::parse(mod_id)?;
            let index = thunderstore::fetch_index(db)?;
            let (_package, version) = thunderstore::resolve(&mod_ref, &index)?;
            (
                mod_ref.mod_id(),
                version.version_number.clone(),
                version.download_url.clone(),
            )
        }
        ModSource::Nexus => {
            let game_scoped_id = mod_id
                .strip_prefix(source::NEXUS_PREFIX)
                .expect("ModSource::Nexus implies the nexus: prefix");
            let api_key = nexus_api_key(db)?;
            let nexus_mod = nexus::fetch_mod(&api_key, game_scoped_id)?;
            let (download_url, version) = nexus::resolve_download(&api_key, &nexus_mod.id)?;
            (mod_id.to_string(), version, download_url)
        }
        ModSource::Local => anyhow::bail!(
            "'{mod_id}' is a locally uploaded mod and can't be (re-)added by id; \
             upload a new .zip instead"
        ),
    };

    let global_dir = ensure_global_mod(
        paths,
        db,
        &canonical_mod_id,
        &version,
        ModPayload::Download(download_url),
    )?;
    link_and_record(db, &mut instance, &canonical_mod_id, &version, &global_dir)?;

    Ok(())
}

/// Installs a user-uploaded mod `.zip` into the global store under a freshly
/// generated `local:` mod id (a bare zip carries no identifying metadata, so
/// `name`/`version` come from the uploader) and links it into `server_name`.
/// `zip_path` is removed once this returns, whether it succeeds or not.
/// Returns the generated mod id.
pub fn add_local(
    paths: &Paths,
    db: &Db,
    server_name: &str,
    name: &str,
    version: &str,
    zip_path: &Path,
) -> Result<String> {
    let _cleanup = CleanupFile(zip_path);

    let mut instance = Instance::load_existing(paths, db, server_name)?;
    if lifecycle::is_running(&instance)? {
        return Err(InstanceError::ModsLocked(server_name.to_string()).into());
    }
    ensure_bepinex(db, &mut instance)?;

    let mod_id = source::make_local_mod_id(name);
    let global_dir = ensure_global_mod(
        paths,
        db,
        &mod_id,
        version,
        ModPayload::LocalFile(zip_path.to_path_buf()),
    )?;
    link_and_record(db, &mut instance, &mod_id, version, &global_dir)?;

    Ok(mod_id)
}

/// Bootstraps BepInEx into `instance` if it isn't there yet — shared by
/// [`add`] and [`add_local`], both of which need it present before linking a
/// mod's plugin directory in.
fn ensure_bepinex(db: &Db, instance: &mut Instance) -> Result<()> {
    if !instance.state.bepinex_installed {
        tracing::info!(
            instance = instance.state.name,
            "BepInEx not installed yet; bootstrapping"
        );
        bepinex::bootstrap(db, &instance.dir)?;
        instance.state.bepinex_installed = true;
        instance.save(db)?;
    }
    Ok(())
}

/// Reads the configured Nexus Mods API key, or a clear error if none has
/// been set yet (see `crate::web::routes::settings`). Also used directly by
/// the web layer's Nexus lookup/trending routes, which need a key before
/// they can call the Nexus API at all.
pub(crate) fn nexus_api_key(db: &Db) -> Result<String> {
    match crate::db::settings::get(db, crate::db::settings::NEXUS_API_KEY)? {
        Some(key) => Ok(key),
        None => Err(nexus::NexusError::ApiKeyMissing.into()),
    }
}

/// Symlinks a mod into an instance and records/persists its `InstalledMod`
/// entry — the common tail of both [`add`] and [`add_local`].
fn link_and_record(
    db: &Db,
    instance: &mut Instance,
    mod_id: &str,
    version: &str,
    global_dir: &Path,
) -> Result<()> {
    link_into_instance(&instance.dir, mod_id, global_dir)?;

    let entry = InstalledMod {
        mod_id: mod_id.to_string(),
        version: version.to_string(),
        installed_at: Utc::now(),
        enabled: true,
        pinned: false,
    };
    instance
        .state
        .installed_mods
        .retain(|m| m.mod_id != entry.mod_id);
    instance.state.installed_mods.push(entry);
    instance.save(db)
}

pub fn update(paths: &Paths, db: &Db, server_name: &str) -> Result<()> {
    let mut instance = Instance::load_existing(paths, db, server_name)?;

    // Mod ids are collected up front (rather than cloning the whole
    // `Vec<InstalledMod>`) so the loop can still mutate `installed_mods` by
    // id lookup without holding a borrow across the loop body.
    let mod_ids: Vec<String> = instance
        .state
        .installed_mods
        .iter()
        .filter(|m| !m.pinned)
        .map(|m| m.mod_id.clone())
        .collect();

    // Fetched lazily, and only once: an instance with no Thunderstore mods
    // (e.g. only Nexus/local ones) shouldn't pay for a Thunderstore index
    // fetch it'll never use.
    let index = if mod_ids
        .iter()
        .any(|id| source::mod_source(id) == ModSource::Thunderstore)
    {
        Some(thunderstore::fetch_index(db)?)
    } else {
        None
    };

    let mut any_updated = false;
    for mod_id in mod_ids {
        // Nothing to update a locally uploaded mod against — it has no
        // external source to check for a newer version.
        if source::mod_source(&mod_id) == ModSource::Local {
            continue;
        }

        let installed_idx = instance
            .state
            .installed_mods
            .iter()
            .position(|m| m.mod_id == mod_id)
            .expect("mod_id came from installed_mods and is never removed mid-loop");
        let installed = &instance.state.installed_mods[installed_idx];
        // A disabled mod stays disabled: just record the new version so a
        // later `enable` links to it; don't touch `plugins/<mod_id>`.
        let enabled = installed.enabled;

        let (latest_version, download_url) = match source::mod_source(&mod_id) {
            ModSource::Thunderstore => {
                let mod_ref = ModRef::parse(&mod_id)?;
                let index = index
                    .as_ref()
                    .expect("index is fetched above whenever a Thunderstore mod is present");
                let (_package, latest) = thunderstore::resolve(&mod_ref, index)?;
                (latest.version_number.clone(), latest.download_url.clone())
            }
            ModSource::Nexus => {
                let game_scoped_id = mod_id
                    .strip_prefix(source::NEXUS_PREFIX)
                    .expect("ModSource::Nexus implies the nexus: prefix");
                let api_key = nexus_api_key(db)?;
                let nexus_mod = nexus::fetch_mod(&api_key, game_scoped_id)?;
                let (download_url, version) = nexus::resolve_download(&api_key, &nexus_mod.id)?;
                (version, download_url)
            }
            ModSource::Local => unreachable!("local mods are skipped above"),
        };

        if latest_version == installed.version {
            continue;
        }

        tracing::info!(
            instance = server_name,
            mod_id = mod_id,
            from = installed.version,
            to = latest_version,
            "updating mod"
        );

        let global_dir = ensure_global_mod(
            paths,
            db,
            &mod_id,
            &latest_version,
            ModPayload::Download(download_url),
        )?;
        if enabled {
            link_into_instance(&instance.dir, &mod_id, &global_dir)?;
        }

        let entry = &mut instance.state.installed_mods[installed_idx];
        entry.version = latest_version;
        entry.installed_at = Utc::now();
        any_updated = true;
    }

    if any_updated {
        instance.save(db)?;
    } else {
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
pub fn list(paths: &Paths, db: &Db, server_name: &str) -> Result<Vec<InstalledMod>> {
    let instance = Instance::load_existing(paths, db, server_name)?;
    Ok(instance.state.installed_mods)
}

#[derive(Serialize)]
struct ModpackManifestEntry {
    mod_id: String,
    version: String,
}

const MODPACK_README: &str = "\
This ModPack contains the mods currently enabled on this Valheim server.

To install it on your game client:
1. Make sure BepInEx is already installed for your Valheim client (this pack
   does not include it).
2. Extract this zip's `BepInEx` folder into your Valheim install directory,
   merging with the existing `BepInEx/plugins` folder.
3. Make sure every mod matches the version listed in manifest.json — a
   version mismatch with the server can cause connection issues.
";

/// Builds a downloadable zip of every currently-enabled mod's files for an
/// instance, laid out as `BepInEx/plugins/<mod_id>/...` so it can be
/// extracted straight into a player's client BepInEx install. Reads
/// straight from the shared global mod store (`paths.mod_version_dir`) — the same
/// files an instance itself symlinks in — so it works the same regardless
/// of whether a mod came from Thunderstore, Nexus, or a manual upload.
pub fn build_modpack(paths: &Paths, db: &Db, server_name: &str) -> Result<Vec<u8>> {
    let instance = Instance::load_existing(paths, db, server_name)?;
    let enabled: Vec<&InstalledMod> = instance
        .state
        .installed_mods
        .iter()
        .filter(|m| m.enabled)
        .collect();
    if enabled.is_empty() {
        anyhow::bail!("'{server_name}' has no enabled mods to include in a ModPack");
    }

    let mut buf = Vec::new();
    let cursor = std::io::Cursor::new(&mut buf);
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();

    writer.start_file("README.txt", options)?;
    writer.write_all(MODPACK_README.as_bytes())?;

    let manifest: Vec<ModpackManifestEntry> = enabled
        .iter()
        .map(|m| ModpackManifestEntry {
            mod_id: m.mod_id.clone(),
            version: m.version.clone(),
        })
        .collect();
    writer.start_file("manifest.json", options)?;
    writer.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

    for m in &enabled {
        let mod_dir = paths.mod_version_dir(&m.mod_id, &m.version);
        if !mod_dir.is_dir() {
            tracing::warn!(
                mod_id = %m.mod_id,
                "enabled mod is missing from the global store; skipping it in the ModPack"
            );
            continue;
        }
        let archive_root = format!("BepInEx/plugins/{}", m.mod_id);
        add_dir_to_modpack_zip(&mut writer, &mod_dir, &mod_dir, &archive_root, options)?;
    }

    writer.finish().context("failed to finalize ModPack zip")?;
    Ok(buf)
}

fn add_dir_to_modpack_zip(
    writer: &mut zip::ZipWriter<std::io::Cursor<&mut Vec<u8>>>,
    root: &Path,
    dir: &Path,
    archive_root: &str,
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
            add_dir_to_modpack_zip(writer, root, &path, archive_root, options)?;
        } else {
            writer.start_file(format!("{archive_root}/{relative}"), options)?;
            let mut file = std::fs::File::open(&path)?;
            std::io::copy(&mut file, writer)?;
        }
    }
    Ok(())
}

pub fn remove(paths: &Paths, db: &Db, server_name: &str, mod_id: &str) -> Result<()> {
    let mut instance = Instance::load_existing(paths, db, server_name)?;
    if lifecycle::is_running(&instance)? {
        return Err(InstanceError::ModsLocked(server_name.to_string()).into());
    }

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
    instance.save(db)?;

    Ok(())
}

/// Symlinks or unlinks a mod's entry under `plugins/` and flips its stored
/// `enabled` flag. Returns whether anything actually changed (false if the
/// mod was already in the requested state).
pub fn set_enabled(
    paths: &Paths,
    db: &Db,
    server_name: &str,
    mod_id: &str,
    enabled: bool,
) -> Result<bool> {
    let mut instance = Instance::load_existing(paths, db, server_name)?;

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

    if lifecycle::is_running(&instance)? {
        return Err(InstanceError::ModsLocked(server_name.to_string()).into());
    }

    if enabled {
        let version = instance
            .state
            .installed_mods
            .iter()
            .find(|m| m.mod_id == mod_id)
            .map(|m| m.version.as_str())
            .expect("installed mod was found above");
        let global_dir = paths.mod_version_dir(mod_id, version);
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
    instance.save(db)?;

    Ok(true)
}

/// Switches one instance to an already-cached version and pins it so a later
/// "Update all" does not immediately undo the explicit choice.
pub fn select_version(
    paths: &Paths,
    db: &Db,
    server_name: &str,
    mod_id: &str,
    version: &str,
) -> Result<bool> {
    validate_version(version)?;
    let mut instance = Instance::load_existing(paths, db, server_name)?;
    let installed = instance
        .state
        .installed_mods
        .iter()
        .find(|m| m.mod_id == mod_id)
        .ok_or_else(|| anyhow::anyhow!("mod '{mod_id}' is not installed on '{server_name}'"))?;
    if installed.version == version && installed.pinned {
        return Ok(false);
    }
    if lifecycle::is_running(&instance)? && installed.version != version {
        return Err(InstanceError::ModsLocked(server_name.to_string()).into());
    }
    if !crate::db::global_mods::contains(db, mod_id, version)? {
        anyhow::bail!("version '{version}' of '{mod_id}' is not in the shared mod store");
    }
    let version_dir = paths.mod_version_dir(mod_id, version);
    if !version_dir.is_dir() {
        anyhow::bail!(
            "version '{version}' of '{mod_id}' is missing from the shared mod store ({})",
            version_dir.display()
        );
    }

    if installed.enabled && installed.version != version {
        link_into_instance(&instance.dir, mod_id, &version_dir)?;
    }
    let entry = instance
        .state
        .installed_mods
        .iter_mut()
        .find(|m| m.mod_id == mod_id)
        .expect("installed mod was found above");
    entry.version = version.to_string();
    entry.installed_at = Utc::now();
    entry.pinned = true;
    instance.save(db)?;
    Ok(true)
}

/// Controls whether update operations may move this instance's mod to a
/// newer version. Pinning is metadata-only and is safe while running.
pub fn set_pinned(
    paths: &Paths,
    db: &Db,
    server_name: &str,
    mod_id: &str,
    pinned: bool,
) -> Result<bool> {
    let mut instance = Instance::load_existing(paths, db, server_name)?;
    let entry = instance
        .state
        .installed_mods
        .iter_mut()
        .find(|m| m.mod_id == mod_id)
        .ok_or_else(|| anyhow::anyhow!("mod '{mod_id}' is not installed on '{server_name}'"))?;
    if entry.pinned == pinned {
        return Ok(false);
    }
    entry.pinned = pinned;
    instance.save(db)?;
    Ok(true)
}

#[derive(Debug, Clone, Serialize)]
pub struct GlobalModInstanceEntry {
    pub instance: String,
    pub version: String,
    pub enabled: bool,
    pub pinned: bool,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlobalMod {
    pub mod_id: String,
    /// Every payload currently cached in the shared store, newest download
    /// first. Instance entries identify which one they use.
    pub stored_versions: Vec<String>,
    /// Icon URL, best-effort looked up from the Thunderstore index. `None`
    /// if the index couldn't be fetched or the mod isn't listed there.
    pub icon: Option<String>,
    pub instances: Vec<GlobalModInstanceEntry>,
}

/// Aggregates the shared mod store (`paths.mods_dir()`) with what each
/// instance currently has installed, for a cross-instance view. A mod
/// present in the store but installed on zero instances is still included —
/// it's an orphaned download taking up disk space. Also attaches an icon per
/// mod via a best-effort Thunderstore index lookup: a fetch failure (offline,
/// Thunderstore down) doesn't fail the whole listing, mods just show without
/// icons.
pub fn list_global(paths: &Paths, db: &Db) -> Result<Vec<GlobalMod>> {
    let mut mods: BTreeMap<String, GlobalMod> = BTreeMap::new();

    for (mod_id, version) in crate::db::global_mods::list_all(db)? {
        mods.entry(mod_id.clone())
            .or_insert_with(|| GlobalMod {
                mod_id,
                stored_versions: Vec::new(),
                icon: None,
                instances: Vec::new(),
            })
            .stored_versions
            .push(version);
    }

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
            mods.entry(mod_id.clone()).or_insert_with(|| GlobalMod {
                mod_id: mod_id.clone(),
                stored_versions: Vec::new(),
                icon: None,
                instances: Vec::new(),
            });
        }
    }

    for instance in crate::instance::list_all(paths, db)? {
        let running = crate::instance::lifecycle::is_running(&instance)?;
        for installed in &instance.state.installed_mods {
            mods.entry(installed.mod_id.clone())
                .or_insert_with(|| GlobalMod {
                    mod_id: installed.mod_id.clone(),
                    stored_versions: Vec::new(),
                    icon: None,
                    instances: Vec::new(),
                })
                .instances
                .push(GlobalModInstanceEntry {
                    instance: instance.state.name.clone(),
                    version: installed.version.clone(),
                    enabled: installed.enabled,
                    pinned: installed.pinned,
                    running,
                });
        }
    }

    let index = thunderstore::fetch_index(db).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "failed to fetch Thunderstore index; mods will show without icons");
        Vec::new()
    });
    let mut mods: Vec<GlobalMod> = mods.into_values().collect();
    for m in &mut mods {
        // Nexus/local mods have no bulk index to look an icon up in, and
        // this view is polled every few seconds by the dashboard — not
        // worth a live per-mod Nexus call on every poll. They just show
        // without an icon here (the one-off discovery-time preview from
        // `nexus::fetch_mod`/`fetch_trending` does carry a real icon).
        if source::mod_source(&m.mod_id) != ModSource::Thunderstore {
            continue;
        }
        let version_for_icon = m
            .stored_versions
            .first()
            .cloned()
            .or_else(|| m.instances.first().map(|i| i.version.clone()));
        if let Some(version) = version_for_icon {
            m.icon = thunderstore::find_icon(&index, &m.mod_id, &version);
        }
    }

    Ok(mods)
}

/// Removes a mod's payload from the global store. Refuses if any instance
/// still references it — it must be removed per-instance first via `remove`.
pub fn prune_global(paths: &Paths, db: &Db, mod_id: &str) -> Result<()> {
    let in_use = crate::instance::list_all(paths, db)?
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
    crate::db::global_mods::remove(db, mod_id)?;
    Ok(())
}

/// Removes one cached version while preserving the other versions of the
/// same mod. Refuses while any instance still references the exact payload.
pub fn prune_version(paths: &Paths, db: &Db, mod_id: &str, version: &str) -> Result<()> {
    validate_version(version)?;
    let in_use = crate::instance::list_all(paths, db)?
        .into_iter()
        .any(|instance| {
            instance
                .state
                .installed_mods
                .iter()
                .any(|m| m.mod_id == mod_id && m.version == version)
        });
    if in_use {
        anyhow::bail!(
            "version '{version}' of '{mod_id}' is still installed on at least one instance"
        );
    }

    let dir = paths.mod_version_dir(mod_id, version);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("failed to remove {}", dir.display()))?;
    }
    crate::db::global_mods::remove_version(db, mod_id, version)
}

fn active_plugin_dir(instance_dir: &Path, mod_id: &str) -> PathBuf {
    paths::instance_bepinex_dir(instance_dir)
        .join("plugins")
        .join(mod_id)
}

/// Removes the wrapped directory when dropped, regardless of whether the
/// scope exited normally or via an early `?` return — used to guarantee
/// staging/tmp dirs never leak on a failed install step.
pub(super) struct CleanupDir<'a>(pub &'a Path);

impl Drop for CleanupDir<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0);
    }
}

/// Removes the wrapped file when dropped, regardless of whether the scope
/// exited normally or via an early `?` return — used so an uploaded temp
/// zip is guaranteed cleaned up once `add_local` finishes with it, success
/// or failure.
pub(super) struct CleanupFile<'a>(pub &'a Path);

impl Drop for CleanupFile<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

/// Where a mod's zip payload comes from for [`ensure_global_mod`] — a
/// registry download URL (Thunderstore, Nexus) or an already-on-disk file
/// (a user upload). Only the "get the zip onto disk" step differs between
/// the two; extraction and installation into the shared store are identical.
enum ModPayload {
    Download(String),
    LocalFile(PathBuf),
}

/// Ensures one immutable `(mod_id, version)` payload exists in the shared
/// store, downloading it only when that exact version is absent.
fn ensure_global_mod(
    paths: &Paths,
    db: &Db,
    mod_id: &str,
    version_number: &str,
    payload: ModPayload,
) -> Result<PathBuf> {
    validate_version(version_number)?;
    let final_dir = paths.mod_version_dir(mod_id, version_number);
    if crate::db::global_mods::contains(db, mod_id, version_number)? && final_dir.is_dir() {
        return Ok(final_dir);
    }

    // Suffixed with a fresh uuid (not just the mod_id) so two concurrent
    // installs of the same mod for different instances — e.g. installing to
    // several instances at once from the dashboard — don't share, and race
    // on cleaning up, the same staging directory.
    let staging_dir =
        paths
            .mods_dir()
            .join(format!(".install-tmp-{}-{}", mod_id, uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&staging_dir)?;
    let _cleanup = CleanupDir(&staging_dir);

    let zip_path = match payload {
        ModPayload::Download(url) => thunderstore::download_zip(&url, &staging_dir)?,
        ModPayload::LocalFile(path) => path,
    };
    let extract_dir = staging_dir.join("extracted");
    extract_zip_to_dir(&zip_path, &extract_dir)?;
    let source_root = effective_source_root(&extract_dir)?;

    if let Some(parent) = final_dir.parent() {
        std::fs::create_dir_all(parent)?;
        std::fs::write(parent.join(VERSIONED_STORE_MARKER), [])?;
    }
    if final_dir.is_dir() {
        std::fs::remove_dir_all(&final_dir).with_context(|| {
            format!(
                "failed to remove incomplete copy at {}",
                final_dir.display()
            )
        })?;
    }
    copy_dir_contents_excluding_metadata(&source_root, &final_dir)
        .with_context(|| format!("failed to install mod '{mod_id}'"))?;
    crate::db::global_mods::insert(db, mod_id, version_number)?;

    Ok(final_dir)
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
    let mut entries: Vec<_> =
        std::fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.retain(|e| {
        !METADATA_ENTRIES
            .iter()
            .any(|m| e.file_name().to_string_lossy() == *m)
    });
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

    /// Writes a zip at `zip_path` with one entry (`entry_name` -> `content`),
    /// using the raw `zip` crate writer so an unsafe entry name (e.g.
    /// containing `..`) can be tested regardless of any higher-level
    /// sanitization elsewhere in this module.
    fn write_zip(zip_path: &Path, entry_name: &str, content: &[u8]) {
        use std::io::Write as _;
        let file = std::fs::File::create(zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(entry_name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content).unwrap();
        writer.finish().unwrap();
    }

    #[test]
    fn extract_zip_to_dir_never_writes_outside_the_destination() {
        let dir = temp_dir("zip-slip");
        let zip_path = dir.join("malicious.zip");
        write_zip(&zip_path, "../evil.txt", b"pwned");

        let dest = dir.join("dest");
        // Whether extraction errors out or silently drops the unsafe entry is
        // an implementation detail of the `zip` crate; what must hold either
        // way is that nothing lands outside `dest`.
        let _ = extract_zip_to_dir(&zip_path, &dest);

        assert!(!dir.join("evil.txt").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn temp_paths(label: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!(
            "odin-mods-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        }
    }

    fn temp_paths_and_db(label: &str) -> (Paths, Db) {
        let paths = temp_paths(label);
        let db = Db::open(&paths).unwrap();
        (paths, db)
    }

    fn cache_test_version(
        paths: &Paths,
        db: &Db,
        mod_id: &str,
        version: &str,
        content: &[u8],
    ) -> PathBuf {
        let zip_path = paths
            .data_dir
            .join(format!("{mod_id}-{version}-{}.zip", uuid::Uuid::new_v4()));
        write_zip(&zip_path, "plugin.dll", content);
        let dir = ensure_global_mod(
            paths,
            db,
            mod_id,
            version,
            ModPayload::LocalFile(zip_path.clone()),
        )
        .unwrap();
        std::fs::remove_file(zip_path).unwrap();
        dir
    }

    #[test]
    fn global_store_retains_multiple_versions_of_the_same_mod() {
        let (paths, db) = temp_paths_and_db("multiple-versions");
        let first = cache_test_version(&paths, &db, "owner-mod", "1.0.0", b"one");
        let second = cache_test_version(&paths, &db, "owner-mod", "2.0.0", b"two");

        assert_eq!(std::fs::read(first.join("plugin.dll")).unwrap(), b"one");
        assert_eq!(std::fs::read(second.join("plugin.dll")).unwrap(), b"two");
        assert_eq!(
            crate::db::global_mods::list_versions(&db, "owner-mod")
                .unwrap()
                .len(),
            2
        );
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn selecting_a_cached_version_relinks_and_pins_only_that_instance() {
        let (paths, db) = temp_paths_and_db("select-version");
        let first = cache_test_version(&paths, &db, "owner-mod", "1.0.0", b"one");
        let second = cache_test_version(&paths, &db, "owner-mod", "2.0.0", b"two");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        link_and_record(&db, &mut instance, "owner-mod", "1.0.0", &first).unwrap();
        let mut other = crate::instance::Instance::create(&paths, &db, "other-server").unwrap();
        link_and_record(&db, &mut other, "owner-mod", "1.0.0", &first).unwrap();

        select_version(&paths, &db, "my-server", "owner-mod", "2.0.0").unwrap();

        let reloaded = crate::instance::Instance::load_existing(&paths, &db, "my-server").unwrap();
        assert_eq!(reloaded.state.installed_mods[0].version, "2.0.0");
        assert!(reloaded.state.installed_mods[0].pinned);
        assert_eq!(
            std::fs::read_link(active_plugin_dir(&reloaded.dir, "owner-mod")).unwrap(),
            second
        );
        let other = crate::instance::Instance::load_existing(&paths, &db, "other-server").unwrap();
        assert_eq!(other.state.installed_mods[0].version, "1.0.0");
        assert_eq!(
            std::fs::read_link(active_plugin_dir(&other.dir, "owner-mod")).unwrap(),
            first
        );
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn update_skips_pinned_registry_mods_without_a_network_call() {
        let (paths, db) = temp_paths_and_db("update-skips-pinned");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "owner-mod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
            pinned: true,
        });
        instance.save(&db).unwrap();

        update(&paths, &db, "my-server").unwrap();

        let reloaded = crate::instance::Instance::load_existing(&paths, &db, "my-server").unwrap();
        assert_eq!(reloaded.state.installed_mods[0].version, "1.0.0");
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn legacy_store_migration_preserves_payload_and_repairs_instance_link() {
        let (paths, db) = temp_paths_and_db("legacy-store-migration");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "owner-mod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
            pinned: false,
        });
        instance.save(&db).unwrap();
        crate::db::global_mods::insert(&db, "owner-mod", "2.0.0").unwrap();
        let legacy_dir = paths.mod_dir("owner-mod");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("plugin.dll"), b"legacy").unwrap();
        link_into_instance(&instance.dir, "owner-mod", &legacy_dir).unwrap();

        migrate_legacy_store(&paths, &db).unwrap();

        let version_dir = paths.mod_version_dir("owner-mod", "2.0.0");
        assert_eq!(
            std::fs::read(version_dir.join("plugin.dll")).unwrap(),
            b"legacy"
        );
        assert!(!legacy_dir.join("plugin.dll").exists());
        assert_eq!(
            std::fs::read_link(active_plugin_dir(&instance.dir, "owner-mod")).unwrap(),
            version_dir
        );
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn update_skips_local_mods_without_any_network_call() {
        let (paths, db) = temp_paths_and_db("update-skips-local");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "local:my-mod-abcd1234".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
            pinned: false,
        });
        instance.save(&db).unwrap();

        // Would hang/fail on a real network call if `update` didn't skip
        // `local:` mods before ever fetching the Thunderstore index.
        update(&paths, &db, "my-server").unwrap();

        let reloaded = crate::instance::Instance::load_existing(&paths, &db, "my-server").unwrap();
        assert_eq!(reloaded.state.installed_mods[0].version, "1.0.0");
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn add_local_installs_and_links_an_uploaded_zip() {
        let (paths, db) = temp_paths_and_db("add-local");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        instance.state.bepinex_installed = true;
        instance.save(&db).unwrap();

        let upload_dir = temp_dir("add-local-upload");
        let zip_path = upload_dir.join("upload.zip");
        write_zip(&zip_path, "plugin.dll", b"fake plugin bytes");

        let mod_id =
            add_local(&paths, &db, "my-server", "My Cool Mod", "1.0.0", &zip_path).unwrap();
        assert!(mod_id.starts_with("local:my-cool-mod-"));

        // `add_local` cleans up the source zip once it's done with it.
        assert!(!zip_path.exists());

        let instance = crate::instance::Instance::load_existing(&paths, &db, "my-server").unwrap();
        assert_eq!(instance.state.installed_mods.len(), 1);
        let installed = &instance.state.installed_mods[0];
        assert_eq!(installed.mod_id, mod_id);
        assert_eq!(installed.version, "1.0.0");
        assert!(installed.enabled);

        let global_dir = paths.mod_version_dir(&installed.mod_id, &installed.version);
        assert!(global_dir.join("plugin.dll").is_file());
        assert!(crate::db::global_mods::contains(&db, &installed.mod_id, "1.0.0").unwrap());

        let link = active_plugin_dir(&instance.dir, &installed.mod_id);
        assert_eq!(std::fs::read_link(&link).unwrap(), global_dir);

        // A subsequent update is a no-op for a `local:` mod (nothing to
        // check it against), not an error.
        update(&paths, &db, "my-server").unwrap();
        let after_update =
            crate::instance::Instance::load_existing(&paths, &db, "my-server").unwrap();
        assert_eq!(after_update.state.installed_mods[0].version, "1.0.0");

        std::fs::remove_dir_all(&paths.data_dir).ok();
        std::fs::remove_dir_all(&upload_dir).ok();
    }

    #[test]
    fn build_modpack_zips_only_enabled_mods_with_manifest_and_readme() {
        let (paths, db) = temp_paths_and_db("build-modpack");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "author-EnabledMod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
            pinned: false,
        });
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "author-DisabledMod".to_string(),
            version: "2.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: false,
            pinned: false,
        });
        instance.save(&db).unwrap();

        std::fs::create_dir_all(paths.mod_version_dir("author-EnabledMod", "1.0.0")).unwrap();
        std::fs::write(
            paths
                .mod_version_dir("author-EnabledMod", "1.0.0")
                .join("plugin.dll"),
            b"enabled plugin bytes",
        )
        .unwrap();
        std::fs::create_dir_all(paths.mod_version_dir("author-DisabledMod", "2.0.0")).unwrap();
        std::fs::write(
            paths
                .mod_version_dir("author-DisabledMod", "2.0.0")
                .join("plugin.dll"),
            b"disabled plugin bytes",
        )
        .unwrap();

        let zip_bytes = build_modpack(&paths, &db, "my-server").unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();

        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"manifest.json".to_string()));
        assert!(names.contains(&"README.txt".to_string()));
        assert!(names.contains(&"BepInEx/plugins/author-EnabledMod/plugin.dll".to_string()));
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("BepInEx/plugins/author-DisabledMod/")),
            "disabled mod must not be included in the ModPack"
        );

        let mut manifest = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("manifest.json").unwrap(),
            &mut manifest,
        )
        .unwrap();
        assert!(manifest.contains("author-EnabledMod"));
        assert!(!manifest.contains("author-DisabledMod"));

        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn build_modpack_fails_when_no_mods_are_enabled() {
        let (paths, db) = temp_paths_and_db("build-modpack-empty");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "author-DisabledMod".to_string(),
            version: "2.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: false,
            pinned: false,
        });
        instance.save(&db).unwrap();

        assert!(build_modpack(&paths, &db, "my-server").is_err());
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    /// Marks `instance` as running by recording a real child process's
    /// `(pid, pid_started_at)` — `lifecycle::is_running` always does a live
    /// syscall, so faking "running" needs an actual process, same as
    /// `process.rs`'s own liveness test.
    fn mark_running(instance: &mut crate::instance::Instance, db: &Db) -> std::process::Child {
        let child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let pid = child.id();
        let started_at = crate::instance::process::start_time_of(pid).unwrap();
        instance.state.pid = Some(pid);
        instance.state.pid_started_at = Some(started_at);
        instance.save(db).unwrap();
        child
    }

    #[test]
    fn set_enabled_is_blocked_while_running_but_no_op_still_succeeds() {
        let (paths, db) = temp_paths_and_db("set-enabled-running");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "author-SomeMod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
            pinned: false,
        });
        instance.save(&db).unwrap();
        let mut child = mark_running(&mut instance, &db);

        // No-op (already enabled) must still succeed while running.
        assert!(!set_enabled(&paths, &db, "my-server", "author-SomeMod", true).unwrap());

        // An actual state change must be refused.
        let err = set_enabled(&paths, &db, "my-server", "author-SomeMod", false).unwrap_err();
        assert!(matches!(
            err.downcast_ref::<InstanceError>(),
            Some(InstanceError::ModsLocked(_))
        ));

        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn remove_is_blocked_while_instance_is_running() {
        let (paths, db) = temp_paths_and_db("remove-running");
        let mut instance = crate::instance::Instance::create(&paths, &db, "my-server").unwrap();
        instance.state.installed_mods.push(InstalledMod {
            mod_id: "author-SomeMod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
            pinned: false,
        });
        instance.save(&db).unwrap();
        let mut child = mark_running(&mut instance, &db);

        let err = remove(&paths, &db, "my-server", "author-SomeMod").unwrap_err();
        assert!(matches!(
            err.downcast_ref::<InstanceError>(),
            Some(InstanceError::ModsLocked(_))
        ));

        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }
}
