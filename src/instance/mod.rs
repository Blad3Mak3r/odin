pub mod lifecycle;
pub mod state;

use std::path::PathBuf;

use anyhow::{Context, Result};
use thiserror::Error;

use crate::cli::validate_instance_name;
use crate::paths::{self, Paths};
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
    #[error("instance '{0}' does not exist; run `valheim start {0}` to create it")]
    NotFound(String),
    #[error("invalid server name: {0}")]
    InvalidName(String),
}

pub struct Instance {
    pub dir: PathBuf,
    pub state: InstanceState,
}

impl Instance {
    pub fn state_file(&self) -> PathBuf {
        paths::instance_state_file(&self.dir)
    }

    pub fn save(&self) -> Result<()> {
        self.state.save(&self.state_file())
    }

    /// Loads an existing instance, or None if it hasn't been created yet.
    pub fn load(paths: &Paths, name: &str) -> Result<Option<Self>> {
        validate_instance_name(name).map_err(InstanceError::InvalidName)?;
        let dir = paths.instance_dir(name);
        let state_file = paths::instance_state_file(&dir);
        if !state_file.is_file() {
            return Ok(None);
        }
        let state = InstanceState::load(&state_file)?;
        Ok(Some(Self { dir, state }))
    }

    pub fn load_existing(paths: &Paths, name: &str) -> Result<Self> {
        Self::load(paths, name)?.ok_or_else(|| InstanceError::NotFound(name.to_string()).into())
    }

    /// Loads an existing instance, or creates a new one with an auto-assigned
    /// port that doesn't collide with any other known instance's recorded port.
    pub fn load_or_create(paths: &Paths, name: &str) -> Result<Self> {
        if let Some(instance) = Self::load(paths, name)? {
            return Ok(instance);
        }
        validate_instance_name(name).map_err(InstanceError::InvalidName)?;

        let used_ports: Vec<u16> = list_all(paths)?.iter().map(|i| i.state.port).collect();
        let mut port = DEFAULT_BASE_PORT;
        while used_ports.contains(&port) {
            port += PORT_STRIDE;
        }

        let dir = paths.instance_dir(name);
        let state = InstanceState::new(name, port);
        let instance = Self { dir, state };
        instance.save()?;
        Ok(instance)
    }
}

/// Lists all instances found under `<data_dir>/servers/*/`. An entry with a
/// missing/corrupt state file is reported as an error alongside the ones that
/// loaded successfully, rather than aborting the whole listing.
pub fn list_all(paths: &Paths) -> Result<Vec<Instance>> {
    let servers_dir = paths.servers_dir();
    if !servers_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut instances = Vec::new();
    let entries = std::fs::read_dir(&servers_dir)
        .with_context(|| format!("failed to read servers dir {}", servers_dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", servers_dir.display()))?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(instance) = Instance::load(paths, &name)? {
            instances.push(instance);
        }
    }
    instances.sort_by(|a, b| a.state.name.cmp(&b.state.name));
    Ok(instances)
}

/// Names (only, not full state) of every instance currently running.
pub fn running_instance_names(paths: &Paths) -> Result<Vec<String>> {
    let mut running = Vec::new();
    for instance in list_all(paths)? {
        if crate::tmux::has_session(&instance.state.tmux_session)? {
            running.push(instance.state.name);
        }
    }
    Ok(running)
}
