//! A persisted, append-only feed of notable events across every instance
//! (created/deleted/started/stopped, mods installed/removed/updated, the
//! server binary installed/updated, players joining/leaving) — what powers
//! the dashboard's global activity panel. Kept in the `activity_events`
//! table so it survives an `odin serve` restart; an in-memory ring buffer
//! plus a broadcast channel serve live readers without re-querying the
//! database on every request.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::db::Db;

const MAX_BUFFERED_EVENTS: usize = 200;
const BROADCAST_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivityKind {
    InstanceCreated,
    InstanceDeleted,
    InstanceStarted,
    InstanceStopped,
    ServerInstalled,
    ModInstalled { mod_id: String },
    ModRemoved { mod_id: String },
    ModsUpdated,
    BackupCreated { backup_id: String },
    BackupRestored { backup_id: String },
    BackupPruned { backup_id: String },
    PlayerJoined { name: String },
    PlayerLeft { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: String,
    pub at: DateTime<Utc>,
    pub instance: Option<String>,
    pub kind: ActivityKind,
}

struct Inner {
    db: Arc<Db>,
    buffer: Mutex<VecDeque<ActivityEvent>>,
    sender: broadcast::Sender<ActivityEvent>,
}

#[derive(Clone)]
pub struct ActivityLog {
    inner: Arc<Inner>,
}

impl ActivityLog {
    /// Loads the most recent events from the database into memory and
    /// prepares to record new ones there. Never fails: a query error just
    /// starts with an empty buffer, since the activity feed is a
    /// convenience, not state anything else depends on.
    pub fn load(db: Arc<Db>) -> Self {
        let buffer: VecDeque<ActivityEvent> = crate::db::activity::recent(&db, MAX_BUFFERED_EVENTS)
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed to load activity history");
                Vec::new()
            })
            .into();
        let (sender, _receiver) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                db,
                buffer: Mutex::new(buffer),
                sender,
            }),
        }
    }

    /// Records a new event: buffers it, persists it to the database, and
    /// broadcasts it to any live subscribers. Does blocking I/O — call from
    /// a blocking context.
    pub fn record(&self, kind: ActivityKind, instance: Option<String>) {
        let event = ActivityEvent {
            id: Uuid::new_v4().to_string(),
            at: Utc::now(),
            instance,
            kind,
        };

        {
            let mut buffer = self
                .inner
                .buffer
                .lock()
                .expect("activity log buffer lock poisoned");
            buffer.push_back(event.clone());
            if buffer.len() > MAX_BUFFERED_EVENTS {
                buffer.pop_front();
            }
        }

        if let Err(e) = crate::db::activity::insert(&self.inner.db, &event) {
            tracing::warn!(error = %e, "failed to persist activity event");
        }

        let _ = self.inner.sender.send(event);
    }

    /// Returns the buffered recent history plus a receiver for events from
    /// this point on, so a client that connects mid-stream still sees
    /// recent context instead of starting from nothing.
    pub fn subscribe(&self) -> (Vec<ActivityEvent>, broadcast::Receiver<ActivityEvent>) {
        let buffer = self
            .inner
            .buffer
            .lock()
            .expect("activity log buffer lock poisoned");
        (
            buffer.iter().cloned().collect(),
            self.inner.sender.subscribe(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_db() -> Arc<Db> {
        let dir = std::env::temp_dir().join(format!(
            "odin-activity-test-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(
            Db::open(&Paths {
                data_dir: dir.clone(),
                config_dir: dir,
            })
            .unwrap(),
        )
    }

    #[test]
    fn recorded_event_is_buffered_and_broadcast() {
        let db = temp_db();
        let log = ActivityLog::load(db);
        let (_history, mut rx) = log.subscribe();

        log.record(ActivityKind::InstanceCreated, Some("my-server".to_string()));

        let (history, _rx) = log.subscribe();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].instance.as_deref(), Some("my-server"));

        let broadcast = rx.try_recv().expect("event should be broadcast");
        assert_eq!(broadcast.instance.as_deref(), Some("my-server"));
    }

    #[test]
    fn reloading_the_log_replays_persisted_events() {
        let db = temp_db();
        {
            let log = ActivityLog::load(db.clone());
            log.record(ActivityKind::ServerInstalled, None);
            log.record(
                ActivityKind::ModInstalled {
                    mod_id: "owner-mod".to_string(),
                },
                Some("my-server".to_string()),
            );
        }

        let reloaded = ActivityLog::load(db);
        let (history, _rx) = reloaded.subscribe();

        assert_eq!(history.len(), 2);
        assert!(matches!(history[0].kind, ActivityKind::ServerInstalled));
        assert!(matches!(history[1].kind, ActivityKind::ModInstalled { .. }));
    }
}
