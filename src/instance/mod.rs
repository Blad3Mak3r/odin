//! Instance lookup, creation, and listing. Each instance's state lives in
//! the database (see `crate::db::instances`); `dir` is the instance's
//! directory under `<data_dir>/servers/<name>/`, still used on disk for
//! save/log/mod files. `lifecycle` handles starting, stopping, and
//! renaming the underlying process and on-disk layout.

pub mod lifecycle;
pub mod lists;
pub mod process;
pub mod state;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;

use crate::cli::validate_instance_name;
use crate::db::Db;
use crate::paths::{self, Paths};
use state::{InstalledMod, InstanceState};

const DEFAULT_BASE_PORT: u16 = 2456;
/// Valheim uses `port`, `port+1` (query), and `port+2` (Steam), so successive
/// auto-assigned instances are spaced 3 ports apart to avoid overlap.
const PORT_STRIDE: u16 = 3;

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("instance '{0}' is already running")]
    AlreadyRunning(String),
    #[error("instance '{0}' is not running")]
    NotRunning(String),
    #[error("instance '{0}' already has a lifecycle transition in progress")]
    TransitionInProgress(String),
    #[error("instance '{0}' does not exist; run `odin start {0}` to create it")]
    NotFound(String),
    #[error("instance '{0}' already exists")]
    AlreadyExists(String),
    #[error("invalid server name: {0}")]
    InvalidName(String),
    #[error("world name cannot be empty")]
    InvalidWorldName,
    #[error("instance '{0}' is running; stop it first to change its mods")]
    ModsLocked(String),
}

pub struct Instance {
    pub dir: PathBuf,
    pub state: InstanceState,
}

impl Instance {
    pub fn save(&self, db: &Db) -> Result<()> {
        crate::db::instances::save(db, &self.state)
    }

    /// Loads an existing instance, or None if it hasn't been created yet.
    pub fn load(paths: &Paths, db: &Db, name: &str) -> Result<Option<Self>> {
        validate_instance_name(name).map_err(InstanceError::InvalidName)?;
        let dir = paths.instance_dir(name);
        Ok(crate::db::instances::load(db, name)?.map(|state| Self { dir, state }))
    }

    pub fn load_existing(paths: &Paths, db: &Db, name: &str) -> Result<Self> {
        Self::load(paths, db, name)?.ok_or_else(|| InstanceError::NotFound(name.to_string()).into())
    }

    /// Loads an existing instance, or creates a new one with an auto-assigned
    /// port that doesn't collide with any other known instance's recorded port.
    pub fn load_or_create(paths: &Paths, db: &Db, name: &str) -> Result<Self> {
        if let Some(instance) = Self::load(paths, db, name)? {
            return Ok(instance);
        }
        Self::create_new(paths, db, name)
    }

    /// Creates a new instance, failing if one with this name already exists.
    /// Unlike `load_or_create`, this never returns an already-existing instance.
    pub fn create(paths: &Paths, db: &Db, name: &str) -> Result<Self> {
        if Self::load(paths, db, name)?.is_some() {
            anyhow::bail!(InstanceError::AlreadyExists(name.to_string()));
        }
        Self::create_new(paths, db, name)
    }

    fn create_new(paths: &Paths, db: &Db, name: &str) -> Result<Self> {
        validate_instance_name(name).map_err(InstanceError::InvalidName)?;

        let port = next_available_port(paths, db)?;

        let dir = paths.instance_dir(name);
        // The DB save below has no filesystem side effect (unlike the old
        // file-based one, which implicitly created this directory as the
        // parent of state.json) — create it explicitly so code that assumes
        // a freshly created instance already has a directory (e.g. `rename`)
        // keeps working.
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create instance dir {}", dir.display()))?;
        let state = InstanceState::new(name, port);
        let instance = Self { dir, state };
        instance.save(db)?;
        Ok(instance)
    }
}

/// Creates a stopped instance from another stopped instance's operational
/// configuration. World data, jobs, backups, logs, remote backup credentials,
/// and backup schedules are deliberately excluded.
pub fn clone_configuration(
    paths: &Paths,
    db: &Db,
    source_name: &str,
    target_name: &str,
    world_name: &str,
) -> Result<Instance> {
    validate_instance_name(target_name).map_err(InstanceError::InvalidName)?;
    if world_name.trim().is_empty() {
        return Err(InstanceError::InvalidWorldName.into());
    }

    let source = Instance::load_existing(paths, db, source_name)?;
    if lifecycle::is_running(&source)? {
        return Err(InstanceError::AlreadyRunning(source_name.to_string()).into());
    }
    if Instance::load(paths, db, target_name)?.is_some() {
        return Err(InstanceError::AlreadyExists(target_name.to_string()).into());
    }

    let target_dir = paths.instance_dir(target_name);
    if target_dir.exists() {
        return Err(InstanceError::AlreadyExists(target_name.to_string()).into());
    }

    let access_lists = [
        (
            lists::ListKind::Admin,
            lists::read(db, &source, lists::ListKind::Admin)?,
        ),
        (
            lists::ListKind::Banned,
            lists::read(db, &source, lists::ListKind::Banned)?,
        ),
        (
            lists::ListKind::Permitted,
            lists::read(db, &source, lists::ListKind::Permitted)?,
        ),
    ];
    let target = clone_state(paths, db, &source.state, target_name, world_name)?;
    let staging_dir = paths
        .servers_dir()
        .join(format!(".clone-{}", uuid::Uuid::new_v4()));

    let prepare_result = (|| -> Result<()> {
        std::fs::create_dir_all(&staging_dir)
            .with_context(|| format!("failed to create {}", staging_dir.display()))?;
        if source.state.bepinex_installed {
            copy_bepinex_configuration(&source.dir, &staging_dir)?;
            crate::mods::link_existing_mods(paths, &staging_dir, &target.state.installed_mods)?;
        }
        for (kind, ids) in &access_lists {
            lists::write_file(&staging_dir, *kind, ids)?;
        }
        Ok(())
    })();
    if let Err(error) = prepare_result {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(error);
    }

    std::fs::rename(&staging_dir, &target_dir).with_context(|| {
        format!(
            "failed to publish cloned instance directory {}",
            target_dir.display()
        )
    })?;
    let db_lists: Vec<_> = access_lists
        .iter()
        .map(|(kind, ids)| (kind.db_value(), ids.as_slice()))
        .collect();
    if let Err(error) = crate::db::instances::save_clone(db, &target.state, &db_lists) {
        let _ = std::fs::remove_dir_all(&target_dir);
        return Err(error);
    }

    Ok(target)
}

fn clone_state(
    paths: &Paths,
    db: &Db,
    source: &InstanceState,
    target_name: &str,
    world_name: &str,
) -> Result<Instance> {
    let mut state = InstanceState::new(target_name, next_available_port(paths, db)?);
    state.world_name = world_name.to_string();
    state.public = source.public;
    state.auto_restart = source.auto_restart;
    state.bepinex_installed = source.bepinex_installed;
    state.bepinex_version = source.bepinex_version.clone();
    state.installed_mods = source
        .installed_mods
        .iter()
        .map(|installed| InstalledMod {
            mod_id: installed.mod_id.clone(),
            version: installed.version.clone(),
            installed_at: chrono::Utc::now(),
            enabled: installed.enabled,
            pinned: installed.pinned,
        })
        .collect();
    Ok(Instance {
        dir: paths.instance_dir(target_name),
        state,
    })
}

fn next_available_port(paths: &Paths, db: &Db) -> Result<u16> {
    let used_ports: Vec<u16> = list_all(paths, db)?.iter().map(|i| i.state.port).collect();
    let mut port = DEFAULT_BASE_PORT;
    while used_ports.contains(&port) {
        port = port
            .checked_add(PORT_STRIDE)
            .context("no available Valheim port block remains")?;
    }
    Ok(port)
}

fn copy_bepinex_configuration(source_dir: &Path, target_dir: &Path) -> Result<()> {
    copy_dir_excluding(
        &paths::instance_bepinex_dir(source_dir),
        &paths::instance_bepinex_dir(target_dir),
        "plugins",
    )?;
    copy_dir_all(
        &source_dir.join("doorstop_libs"),
        &target_dir.join("doorstop_libs"),
    )?;
    std::fs::copy(
        source_dir.join("doorstop_config.ini"),
        target_dir.join("doorstop_config.ini"),
    )
    .context("failed to copy Doorstop configuration")?;
    Ok(())
}

fn copy_dir_excluding(source: &Path, target: &Path, excluded_dir: &str) -> Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == excluded_dir {
            continue;
        }
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &destination)?;
        } else {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn copy_dir_all(source: &Path, target: &Path) -> Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &destination)?;
        } else {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

/// Lists every known instance, ordered by name.
pub fn list_all(paths: &Paths, db: &Db) -> Result<Vec<Instance>> {
    Ok(crate::db::instances::list_all(db)?
        .into_iter()
        .map(|state| Instance {
            dir: paths.instance_dir(&state.name),
            state,
        })
        .collect())
}

/// Names (only, not full state) of every instance currently running.
pub fn running_instance_names(paths: &Paths, db: &Db) -> Result<Vec<String>> {
    let mut running = Vec::new();
    for instance in list_all(paths, db)? {
        if lifecycle::is_running(&instance)? {
            running.push(instance.state.name);
        }
    }
    Ok(running)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::lists::{self, ListKind};

    const ADMIN_ID: &str = "76561197960287930";

    fn temp_context(label: &str) -> (Paths, Db) {
        let dir = std::env::temp_dir().join(format!(
            "odin-instance-clone-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        (paths, db)
    }

    fn configured_source(paths: &Paths, db: &Db) -> Instance {
        let mut source = Instance::create(paths, db, "source").unwrap();
        source.state.public = false;
        source.state.auto_restart = true;
        source.state.bepinex_installed = true;
        source.state.bepinex_version = Some("5.4.2202".to_string());
        source.state.installed_mods = vec![
            InstalledMod {
                mod_id: "author-enabled".to_string(),
                version: "1.0.0".to_string(),
                installed_at: chrono::Utc::now(),
                enabled: true,
                pinned: true,
            },
            InstalledMod {
                mod_id: "author-disabled".to_string(),
                version: "2.0.0".to_string(),
                installed_at: chrono::Utc::now(),
                enabled: false,
                pinned: false,
            },
        ];
        source.save(db).unwrap();

        for installed in &source.state.installed_mods {
            let dir = paths.mod_version_dir(&installed.mod_id, &installed.version);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("plugin.dll"), "plugin").unwrap();
        }
        let bepinex = paths::instance_bepinex_dir(&source.dir);
        std::fs::create_dir_all(bepinex.join("core")).unwrap();
        std::fs::create_dir_all(bepinex.join("config")).unwrap();
        std::fs::create_dir_all(bepinex.join("plugins/source-only")).unwrap();
        std::fs::write(bepinex.join("core/BepInEx.Preloader.dll"), "core").unwrap();
        std::fs::write(bepinex.join("config/plugin.cfg"), "setting = true").unwrap();
        std::fs::write(bepinex.join("plugins/source-only/plugin.dll"), "stale").unwrap();
        std::fs::create_dir_all(source.dir.join("doorstop_libs")).unwrap();
        std::fs::write(
            source.dir.join("doorstop_libs/libdoorstop_x64.so"),
            "doorstop",
        )
        .unwrap();
        std::fs::write(
            source.dir.join("doorstop_config.ini"),
            "target_assembly = BepInEx",
        )
        .unwrap();
        lists::write(db, &source, ListKind::Admin, &[ADMIN_ID.to_string()]).unwrap();
        std::fs::create_dir_all(source.dir.join("backups")).unwrap();
        std::fs::write(source.dir.join("backups/old.zip"), "backup").unwrap();
        source
    }

    #[test]
    fn clone_copies_operational_configuration_but_not_world_data() {
        let (paths, db) = temp_context("complete");
        let source = configured_source(&paths, &db);

        let target = clone_configuration(
            &paths,
            &db,
            &source.state.name,
            "season-two",
            "season-two-world",
        )
        .unwrap();

        assert_eq!(target.state.world_name, "season-two-world");
        assert_eq!(target.state.public, source.state.public);
        assert_eq!(target.state.auto_restart, source.state.auto_restart);
        assert_eq!(target.state.bepinex_version, source.state.bepinex_version);
        assert_ne!(target.state.port, source.state.port);
        assert_ne!(target.state.password, source.state.password);
        assert_eq!(target.state.installed_mods.len(), 2);
        assert_eq!(
            lists::read(&db, &target, ListKind::Admin).unwrap(),
            vec![ADMIN_ID.to_string()]
        );
        assert_eq!(
            std::fs::read_to_string(
                paths::instance_bepinex_dir(&target.dir).join("config/plugin.cfg")
            )
            .unwrap(),
            "setting = true"
        );
        assert!(
            !paths::instance_bepinex_dir(&target.dir)
                .join("plugins/source-only")
                .exists()
        );
        assert_eq!(
            std::fs::read_link(
                paths::instance_bepinex_dir(&target.dir).join("plugins/author-enabled")
            )
            .unwrap(),
            paths.mod_version_dir("author-enabled", "1.0.0")
        );
        assert!(
            !paths::instance_bepinex_dir(&target.dir)
                .join("plugins/author-disabled")
                .exists()
        );
        let save_entries = std::fs::read_dir(target.dir.join("saves"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(save_entries.len(), 3);
        assert!(!target.dir.join("backups").exists());
        assert!(!target.dir.join("logs").exists());
    }

    #[test]
    fn clone_missing_mod_payload_leaves_no_target() {
        let (paths, db) = temp_context("missing-payload");
        let mut source = Instance::create(&paths, &db, "source").unwrap();
        source.state.bepinex_installed = true;
        source.state.installed_mods = vec![InstalledMod {
            mod_id: "author-missing".to_string(),
            version: "1.0.0".to_string(),
            installed_at: chrono::Utc::now(),
            enabled: true,
            pinned: false,
        }];
        source.save(&db).unwrap();
        let bepinex = paths::instance_bepinex_dir(&source.dir);
        std::fs::create_dir_all(bepinex.join("core")).unwrap();
        std::fs::create_dir_all(source.dir.join("doorstop_libs")).unwrap();
        std::fs::write(source.dir.join("doorstop_config.ini"), "config").unwrap();

        let error = match clone_configuration(&paths, &db, "source", "target", "target-world") {
            Ok(_) => panic!("clone should fail when a mod payload is missing"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("missing from the shared store"));
        assert!(Instance::load(&paths, &db, "target").unwrap().is_none());
        assert!(!paths.instance_dir("target").exists());
    }

    #[test]
    fn clone_rejects_a_running_source() {
        let (paths, db) = temp_context("running-source");
        let mut source = Instance::create(&paths, &db, "source").unwrap();
        source.state.pid = Some(std::process::id());
        source.state.pid_started_at =
            Some(crate::instance::process::start_time_of(std::process::id()).unwrap());
        source.save(&db).unwrap();

        let error = match clone_configuration(&paths, &db, "source", "target", "target-world") {
            Ok(_) => panic!("clone should fail while its source is running"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("already running"));
        assert!(Instance::load(&paths, &db, "target").unwrap().is_none());
    }
}
