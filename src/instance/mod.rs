//! Instance lookup, creation, and listing. Each instance's state lives in
//! the database (see `crate::db::instances`); `dir` is the instance's
//! directory under `<data_dir>/servers/<name>/`, still used on disk for
//! save/log/mod files. `lifecycle` handles starting, stopping, and
//! renaming the underlying tmux session and on-disk layout.

pub mod lifecycle;
pub mod lists;
pub mod state;

use std::path::PathBuf;

use anyhow::{Context, Result};
use thiserror::Error;

use crate::cli::validate_instance_name;
use crate::db::Db;
use crate::paths::Paths;
use state::InstanceState;

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
    #[error("instance '{0}' does not exist; run `odin start {0}` to create it")]
    NotFound(String),
    #[error("instance '{0}' already exists")]
    AlreadyExists(String),
    #[error("invalid server name: {0}")]
    InvalidName(String),
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

        let used_ports: Vec<u16> = list_all(paths, db)?.iter().map(|i| i.state.port).collect();
        let mut port = DEFAULT_BASE_PORT;
        while used_ports.contains(&port) {
            port += PORT_STRIDE;
        }

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
        if crate::tmux::has_session(&instance.state.tmux_session)? {
            running.push(instance.state.name);
        }
    }
    Ok(running)
}
