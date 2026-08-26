//! In-memory cache of periodic host/instance resource samples, kept warm by
//! the background telemetry task (`web::spawn_telemetry`) so HTTP handlers
//! read a cheap snapshot instead of recomputing `sysinfo`/`tmux` queries on
//! every request, and so the dashboard can chart recent history instead of
//! only ever seeing the current instant.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Samples kept per series (host, and each instance). At the telemetry
/// task's tick interval (3s), 120 samples covers the last 6 minutes.
const HISTORY_CAPACITY: usize = 120;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ResourceSample {
    pub at: DateTime<Utc>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct HostSnapshot {
    pub cpu_percent: f32,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub disk_available_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct InstanceSnapshot {
    pub running: bool,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Default)]
struct HostState {
    current: HostSnapshot,
    history: VecDeque<ResourceSample>,
}

#[derive(Default)]
struct InstanceState {
    current: InstanceSnapshot,
    history: VecDeque<ResourceSample>,
}

#[derive(Clone)]
pub struct RuntimeRegistry {
    host: Arc<Mutex<HostState>>,
    instances: Arc<Mutex<HashMap<String, InstanceState>>>,
}

impl RuntimeRegistry {
    pub fn new() -> Self {
        Self {
            host: Arc::new(Mutex::new(HostState::default())),
            instances: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn push_host_sample(&self, snapshot: HostSnapshot) {
        let mut host = self.host.lock().expect("runtime host lock poisoned");
        push_capped(
            &mut host.history,
            ResourceSample {
                at: Utc::now(),
                cpu_percent: snapshot.cpu_percent,
                memory_bytes: snapshot.memory_used_bytes,
            },
        );
        host.current = snapshot;
    }

    pub fn host_snapshot(&self) -> HostSnapshot {
        self.host
            .lock()
            .expect("runtime host lock poisoned")
            .current
    }

    pub fn host_history(&self) -> Vec<ResourceSample> {
        self.host
            .lock()
            .expect("runtime host lock poisoned")
            .history
            .iter()
            .copied()
            .collect()
    }

    pub fn push_instance_sample(&self, name: &str, snapshot: InstanceSnapshot) {
        let mut instances = self
            .instances
            .lock()
            .expect("runtime instances lock poisoned");
        let entry = instances.entry(name.to_string()).or_default();
        if snapshot.running {
            push_capped(
                &mut entry.history,
                ResourceSample {
                    at: Utc::now(),
                    cpu_percent: snapshot.cpu_percent,
                    memory_bytes: snapshot.memory_bytes,
                },
            );
        }
        entry.current = snapshot;
    }

    pub fn instance_snapshot(&self, name: &str) -> InstanceSnapshot {
        self.instances
            .lock()
            .expect("runtime instances lock poisoned")
            .get(name)
            .map(|s| s.current)
            .unwrap_or_default()
    }

    pub fn instance_history(&self, name: &str) -> Vec<ResourceSample> {
        self.instances
            .lock()
            .expect("runtime instances lock poisoned")
            .get(name)
            .map(|s| s.history.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Drops cached state for an instance that no longer exists, so a
    /// deleted-then-recreated instance doesn't briefly show stale history.
    pub fn remove_instance(&self, name: &str) {
        self.instances
            .lock()
            .expect("runtime instances lock poisoned")
            .remove(name);
    }
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn push_capped(buf: &mut VecDeque<ResourceSample>, sample: ResourceSample) {
    buf.push_back(sample);
    if buf.len() > HISTORY_CAPACITY {
        buf.pop_front();
    }
}
