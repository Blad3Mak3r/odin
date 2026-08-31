//! In-memory cache of periodic host/instance resource samples, kept warm by
//! the background telemetry task (`web::spawn_telemetry`) so HTTP handlers
//! read a cheap snapshot instead of recomputing `sysinfo` queries on
//! every request, and so the dashboard can chart recent history instead of
//! only ever seeing the current instant.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::db::Db;
use crate::instance::InstanceError;
use crate::web::players::PlayerInfo;

/// Samples kept per series (host, and each instance). At the telemetry
/// task's tick interval (3s), 120 samples covers the last 6 minutes.
const HISTORY_CAPACITY: usize = 120;

/// Live subscribers to `RuntimeRegistry::subscribe_ticks` are dashboard
/// clients polling roughly every tick, so a short buffer is enough to ride
/// out a brief send stall without ever needing much memory.
const TICK_BROADCAST_CAPACITY: usize = 32;

const TRANSITION_BROADCAST_CAPACITY: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceTransition {
    Starting,
    Stopping,
    Restarting,
}

pub type InstanceTransitions = HashMap<String, InstanceTransition>;

/// How often a downsampled sample gets written to `resource_samples` for
/// long-range history — the in-memory buffer above already covers the
/// short term at full resolution, so this only needs to be coarse.
const PERSIST_INTERVAL: chrono::Duration = chrono::Duration::minutes(3);

/// How long a persisted sample is kept before `prune_old_samples` deletes
/// it.
const RETENTION: chrono::Duration = chrono::Duration::days(7);

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ResourceSample {
    pub at: DateTime<Utc>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

impl From<crate::db::resource_samples::ResourceSampleRow> for ResourceSample {
    fn from(row: crate::db::resource_samples::ResourceSampleRow) -> Self {
        Self {
            at: row.at,
            cpu_percent: row.cpu_percent,
            memory_bytes: row.memory_bytes,
        }
    }
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
    /// Whether Valheim has actually finished loading and is accepting
    /// connections, not just "the process/socket exists" — see
    /// `routes::resources::compute_instance_snapshot`. Always `false` when
    /// `running` is (the `Default` derive already gives that for free).
    pub ready: bool,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceResourceEntry {
    pub name: String,
    pub running: bool,
    pub ready: bool,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub players: Vec<PlayerInfo>,
    pub last_saved_at: Option<DateTime<Utc>>,
}

/// One telemetry tick's worth of host + per-instance samples, broadcast to
/// live WebSocket subscribers as it's produced.
#[derive(Debug, Clone, Serialize)]
pub struct ResourcesTick {
    pub host: HostSnapshot,
    pub instances: Vec<InstanceResourceEntry>,
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
    ticks: broadcast::Sender<ResourcesTick>,
    transitions: Arc<Mutex<InstanceTransitions>>,
    transition_events: broadcast::Sender<InstanceTransitions>,
    auto_restart_attempts: Arc<Mutex<HashMap<String, DateTime<Utc>>>>,
    db: Arc<Db>,
    last_persisted_at: Arc<Mutex<Option<DateTime<Utc>>>>,
}

impl RuntimeRegistry {
    pub fn new(db: Arc<Db>) -> Self {
        let (ticks, _receiver) = broadcast::channel(TICK_BROADCAST_CAPACITY);
        let (transition_events, _receiver) = broadcast::channel(TRANSITION_BROADCAST_CAPACITY);
        Self {
            host: Arc::new(Mutex::new(HostState::default())),
            instances: Arc::new(Mutex::new(HashMap::new())),
            ticks,
            transitions: Arc::new(Mutex::new(HashMap::new())),
            transition_events,
            auto_restart_attempts: Arc::new(Mutex::new(HashMap::new())),
            db,
            last_persisted_at: Arc::new(Mutex::new(None)),
        }
    }

    /// Sent once per telemetry tick after every sample in it has already
    /// been pushed into history — subscribers get a consistent, complete
    /// view of that tick rather than a partial one.
    pub fn broadcast_tick(&self, tick: ResourcesTick) {
        let _ = self.ticks.send(tick);
    }

    pub fn subscribe_ticks(&self) -> broadcast::Receiver<ResourcesTick> {
        self.ticks.subscribe()
    }

    /// Marks one instance as transitioning and returns a guard that clears
    /// the marker on every exit path, including cancellation and errors.
    pub fn begin_transition(
        &self,
        name: &str,
        transition: InstanceTransition,
    ) -> Result<InstanceTransitionGuard, InstanceError> {
        let mut transitions = self
            .transitions
            .lock()
            .expect("runtime transitions lock poisoned");
        if transitions.contains_key(name) {
            return Err(InstanceError::TransitionInProgress(name.to_string()));
        }
        transitions.insert(name.to_string(), transition);
        let _ = self.transition_events.send(transitions.clone());
        Ok(InstanceTransitionGuard {
            runtime: self.clone(),
            name: name.to_string(),
        })
    }

    /// Returns one atomic current snapshot plus a receiver for later full
    /// snapshots. Full replacement lets a client recover even if it lagged
    /// over an earlier transition event.
    pub fn subscribe_transitions(
        &self,
    ) -> (
        InstanceTransitions,
        broadcast::Receiver<InstanceTransitions>,
    ) {
        let transitions = self
            .transitions
            .lock()
            .expect("runtime transitions lock poisoned");
        (transitions.clone(), self.transition_events.subscribe())
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

    /// Updates an instance's cached snapshot; returns `true` if `running`
    /// flipped since the previous sample, so the telemetry tick can emit an
    /// activity event only on that transition rather than every tick.
    pub fn push_instance_sample(&self, name: &str, snapshot: InstanceSnapshot) -> bool {
        let mut instances = self
            .instances
            .lock()
            .expect("runtime instances lock poisoned");
        let entry = instances.entry(name.to_string()).or_default();
        let running_changed = entry.current.running != snapshot.running;
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
        running_changed
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

    /// Gates automatic crash-restart attempts: returns `true` (and records
    /// this as the latest attempt) only if the instance hasn't had one
    /// within `cooldown`. Without this, an instance that crashes
    /// immediately on every start (a broken mod, say) would get a fresh
    /// restart attempt on every ~3s telemetry tick forever.
    pub fn should_attempt_auto_restart(&self, name: &str, cooldown: chrono::Duration) -> bool {
        let now = Utc::now();
        let mut attempts = self
            .auto_restart_attempts
            .lock()
            .expect("runtime auto-restart lock poisoned");
        match attempts.get(name) {
            Some(last) if now - *last < cooldown => false,
            _ => {
                attempts.insert(name.to_string(), now);
                true
            }
        }
    }

    /// Whether it's time to write a downsampled sample to durable storage —
    /// call once per telemetry tick, not once per series, since the check
    /// (and the timestamp it records) should be shared across the host and
    /// every instance sampled in the same tick.
    pub fn should_persist_now(&self) -> bool {
        let now = Utc::now();
        let mut last = self
            .last_persisted_at
            .lock()
            .expect("runtime last-persisted lock poisoned");
        match *last {
            Some(prev) if now - prev < PERSIST_INTERVAL => false,
            _ => {
                *last = Some(now);
                true
            }
        }
    }

    /// Persists one series' sample for long-range history. Best-effort: a
    /// write failure is logged, not surfaced, since this is a durability
    /// nicety layered on top of the in-memory history that already serves
    /// the live chart.
    pub fn persist_sample(
        &self,
        instance_name: Option<&str>,
        at: DateTime<Utc>,
        cpu_percent: f32,
        memory_bytes: u64,
    ) {
        if let Err(e) = crate::db::resource_samples::insert(
            &self.db,
            instance_name,
            at,
            cpu_percent,
            memory_bytes,
        ) {
            tracing::warn!(error = %e, "failed to persist resource sample");
        }
    }

    /// Deletes persisted samples older than the retention window. Cheap
    /// enough to call once per persisted tick rather than on its own
    /// schedule.
    pub fn prune_old_samples(&self) {
        if let Err(e) =
            crate::db::resource_samples::prune_older_than(&self.db, Utc::now() - RETENTION)
        {
            tracing::warn!(error = %e, "failed to prune old resource samples");
        }
    }
}

pub struct InstanceTransitionGuard {
    runtime: RuntimeRegistry,
    name: String,
}

impl Drop for InstanceTransitionGuard {
    fn drop(&mut self) {
        let mut transitions = self
            .runtime
            .transitions
            .lock()
            .expect("runtime transitions lock poisoned");
        transitions.remove(&self.name);
        let _ = self.runtime.transition_events.send(transitions.clone());
    }
}

fn push_capped(buf: &mut VecDeque<ResourceSample>, sample: ResourceSample) {
    buf.push_back(sample);
    if buf.len() > HISTORY_CAPACITY {
        buf.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_registry(label: &str) -> RuntimeRegistry {
        let dir = std::env::temp_dir().join(format!(
            "odin-runtime-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Arc::new(
            Db::open(&Paths {
                data_dir: dir.clone(),
                config_dir: dir,
            })
            .unwrap(),
        );
        RuntimeRegistry::new(db)
    }

    #[test]
    fn auto_restart_is_gated_by_cooldown_per_instance() {
        let registry = temp_registry("cooldown");
        let cooldown = chrono::Duration::hours(1);

        assert!(registry.should_attempt_auto_restart("my-server", cooldown));
        assert!(
            !registry.should_attempt_auto_restart("my-server", cooldown),
            "a second attempt within the cooldown should be refused"
        );
        assert!(
            registry.should_attempt_auto_restart("other-server", cooldown),
            "cooldown is tracked per instance, not globally"
        );
    }

    #[test]
    fn auto_restart_is_allowed_again_once_the_cooldown_elapses() {
        let registry = temp_registry("elapsed");
        assert!(registry.should_attempt_auto_restart("my-server", chrono::Duration::zero()));
        assert!(
            registry.should_attempt_auto_restart("my-server", chrono::Duration::zero()),
            "a zero-length cooldown should never block a retry"
        );
    }

    #[test]
    fn should_persist_now_is_gated_by_interval() {
        let registry = temp_registry("persist-gate");
        assert!(
            registry.should_persist_now(),
            "never persisted: due immediately"
        );
        assert!(
            !registry.should_persist_now(),
            "a second check right after should be refused"
        );
    }

    #[test]
    fn transition_guard_broadcasts_full_snapshots_and_clears_on_drop() {
        let registry = temp_registry("transitions");
        let (initial, mut receiver) = registry.subscribe_transitions();
        assert!(initial.is_empty());

        let guard = registry
            .begin_transition("my-server", InstanceTransition::Restarting)
            .unwrap();
        let active = receiver.try_recv().unwrap();
        assert_eq!(
            active.get("my-server"),
            Some(&InstanceTransition::Restarting)
        );
        assert!(matches!(
            registry.begin_transition("my-server", InstanceTransition::Stopping),
            Err(InstanceError::TransitionInProgress(name)) if name == "my-server"
        ));

        drop(guard);
        assert!(receiver.try_recv().unwrap().is_empty());
    }

    #[test]
    fn persist_sample_and_prune_round_trip_through_the_database() {
        let registry = temp_registry("persist-roundtrip");
        let now = Utc::now();

        registry.persist_sample(None, now, 12.5, 4096);
        let host = crate::db::resource_samples::range(
            &registry.db,
            None,
            now - chrono::Duration::seconds(1),
        )
        .unwrap();
        assert_eq!(host.len(), 1);
        assert_eq!(host[0].cpu_percent, 12.5);

        registry.persist_sample(None, now - chrono::Duration::days(10), 1.0, 100);
        registry.prune_old_samples();
        let remaining = crate::db::resource_samples::range(
            &registry.db,
            None,
            now - chrono::Duration::days(30),
        )
        .unwrap();
        assert_eq!(
            remaining.len(),
            1,
            "only the stale sample should have been pruned"
        );
        assert_eq!(remaining[0].cpu_percent, 12.5);
    }
}
