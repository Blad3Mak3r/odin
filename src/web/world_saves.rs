//! Tracks each running instance's most recent "world saved" timestamp, fed
//! by a reachable supervisor's own `console.log` parsing — mirrors
//! `web::players::PlayerRegistry`'s role, just for a single value per
//! instance instead of a list.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

#[derive(Clone, Default)]
pub struct WorldSaveRegistry {
    last_saved: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
}

impl WorldSaveRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, instance: &str) -> Option<DateTime<Utc>> {
        self.last_saved
            .lock()
            .expect("world save registry lock poisoned")
            .get(instance)
            .copied()
    }

    pub fn set(&self, instance: &str, at: DateTime<Utc>) {
        self.last_saved
            .lock()
            .expect("world save registry lock poisoned")
            .insert(instance.to_string(), at);
    }

    /// Drops the tracked timestamp for an instance whose tailer just
    /// stopped, same as `PlayerRegistry::clear_instance` — otherwise a
    /// stopped instance would keep showing a stale "last saved" time
    /// forever.
    pub fn clear_instance(&self, instance: &str) {
        self.last_saved
            .lock()
            .expect("world save registry lock poisoned")
            .remove(instance);
    }
}
