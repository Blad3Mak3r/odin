//! A persisted, append-only feed of notable events across every instance
//! (created/deleted/started/stopped, mods installed/removed/updated, the
//! server binary installed/updated, players joining/leaving) — what powers
//! the dashboard's global activity panel. Kept in `<data_dir>/activity.jsonl`,
//! one JSON object per line, so it survives an `odin serve` restart; an
//! in-memory ring buffer plus a broadcast channel serve live readers without
//! re-reading the file on every request.

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

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
    path: PathBuf,
    buffer: Mutex<VecDeque<ActivityEvent>>,
    sender: broadcast::Sender<ActivityEvent>,
}

#[derive(Clone)]
pub struct ActivityLog {
    inner: Arc<Inner>,
}

impl ActivityLog {
    /// Loads the tail of `data_dir/activity.jsonl` (if present) into memory
    /// and prepares to append new events there. Never fails: a missing or
    /// unreadable log file just starts with an empty buffer, since the
    /// activity feed is a convenience, not state anything else depends on.
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("activity.jsonl");
        let buffer = read_recent_events(&path);
        let (sender, _receiver) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                path,
                buffer: Mutex::new(buffer),
                sender,
            }),
        }
    }

    /// Records a new event: buffers it, appends it to the on-disk log, and
    /// broadcasts it to any live subscribers. Does file I/O — call from a
    /// blocking context.
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

        if let Err(e) = append_event(&self.inner.path, &event) {
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

fn read_recent_events(path: &Path) -> VecDeque<ActivityEvent> {
    let Ok(tail) = crate::commands::logs::read_tail(path, MAX_BUFFERED_EVENTS) else {
        return VecDeque::new();
    };
    tail.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn append_event(path: &Path, event: &ActivityEvent) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let line = serde_json::to_string(event)?;
    writeln!(file, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_data_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "odin-activity-test-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ))
    }

    #[test]
    fn recorded_event_is_buffered_and_broadcast() {
        let dir = temp_data_dir();
        let log = ActivityLog::load(&dir);
        let (_history, mut rx) = log.subscribe();

        log.record(ActivityKind::InstanceCreated, Some("my-server".to_string()));

        let (history, _rx) = log.subscribe();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].instance.as_deref(), Some("my-server"));

        let broadcast = rx.try_recv().expect("event should be broadcast");
        assert_eq!(broadcast.instance.as_deref(), Some("my-server"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reloading_the_log_replays_persisted_events() {
        let dir = temp_data_dir();
        {
            let log = ActivityLog::load(&dir);
            log.record(ActivityKind::ServerInstalled, None);
            log.record(
                ActivityKind::ModInstalled {
                    mod_id: "owner-mod".to_string(),
                },
                Some("my-server".to_string()),
            );
        }

        let reloaded = ActivityLog::load(&dir);
        let (history, _rx) = reloaded.subscribe();

        assert_eq!(history.len(), 2);
        assert!(matches!(history[0].kind, ActivityKind::ServerInstalled));
        assert!(matches!(history[1].kind, ActivityKind::ModInstalled { .. }));

        std::fs::remove_dir_all(&dir).ok();
    }
}
