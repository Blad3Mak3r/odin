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
use crate::game::GameId;
use crate::lock::LockExt;

const MAX_BUFFERED_EVENTS: usize = 200;
const BROADCAST_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivityKind {
    InstanceCreated,
    InstanceCloned {
        source: String,
    },
    InstanceDeleted,
    InstanceStarted,
    InstanceStopped,
    InstanceAutoRestarted,
    ServerInstalled,
    ServerUpdateAvailable {
        installed_build_id: u64,
        latest_build_id: u64,
    },
    ModInstalled {
        mod_id: String,
    },
    ModRemoved {
        mod_id: String,
    },
    ModsUpdated,
    #[serde(rename = "bepinex_updated")]
    BepInExUpdated {
        from_version: Option<String>,
        to_version: String,
    },
    BackupCreated {
        backup_id: String,
    },
    BackupRestored {
        backup_id: String,
    },
    BackupPruned {
        backup_id: String,
    },
    MapWiped,
    FullWiped,
    JobFailed {
        job: String,
        message: String,
    },
    /// An automatic start (crash restart or uptime schedule) that did not
    /// bring the instance up. Manual starts report their failure to the
    /// admin who asked for them and are not recorded.
    StartupFailed {
        reason: String,
    },
    PlayerJoined {
        name: String,
    },
    PlayerLeft {
        name: String,
    },
}

/// Longest failure text kept in an event; what a failure says first is what
/// matters, and the full text stays in the job log.
const MAX_FAILURE_TEXT: usize = 1000;

impl ActivityKind {
    /// A failed background job. The message is cleaned of credentials first,
    /// because events are stored, shown in the dashboard and sent to webhooks.
    pub fn job_failed(job: String, message: &str) -> Self {
        Self::JobFailed {
            job,
            message: failure_text(message),
        }
    }

    /// A failed automatic start; the reason is cleaned like [`Self::job_failed`].
    pub fn startup_failed(reason: &str) -> Self {
        Self::StartupFailed {
            reason: failure_text(reason),
        }
    }
}

fn failure_text(text: &str) -> String {
    crate::redact::secrets(text)
        .chars()
        .take(MAX_FAILURE_TEXT)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: String,
    pub at: DateTime<Utc>,
    pub game: GameId,
    pub instance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
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
        self.record_for(GameId::Valheim, kind, instance);
    }

    /// Records an event for a statically supported game.
    pub fn record_for(&self, game: GameId, kind: ActivityKind, instance: Option<String>) {
        let instance_id = instance.as_deref().and_then(|name| {
            match crate::db::game_instances::identity(&self.inner.db, game, name) {
                Ok(identity) => identity.map(|identity| identity.id),
                Err(error) => {
                    tracing::warn!(%error, %game, %name, "failed to resolve activity instance identity");
                    None
                }
            }
        });
        let event = ActivityEvent {
            id: Uuid::new_v4().to_string(),
            at: Utc::now(),
            game,
            instance,
            instance_id,
            kind,
        };

        {
            let mut buffer = self.inner.buffer.lock_or_recover();
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
        let buffer = self.inner.buffer.lock_or_recover();
        (
            buffer.iter().cloned().collect(),
            self.inner.sender.subscribe(),
        )
    }

    /// Deletes persisted expired events and removes them from the live history.
    pub fn purge_before(&self, before: DateTime<Utc>) -> anyhow::Result<usize> {
        let deleted = crate::db::activity::delete_before(&self.inner.db, before)?;
        let mut buffer = self.inner.buffer.lock_or_recover();
        buffer.retain(|event| event.at >= before);
        Ok(deleted)
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
    fn record_for_includes_the_game_and_instance_identity() {
        let db = temp_db();
        crate::db::instances::save(
            &db,
            &crate::instance::state::InstanceState::new("my-server", 2456),
        )
        .unwrap();
        let log = ActivityLog::load(db);

        log.record_for(
            GameId::Valheim,
            ActivityKind::InstanceCreated,
            Some("my-server".to_string()),
        );

        let (history, _rx) = log.subscribe();
        assert_eq!(history[0].game, GameId::Valheim);
        assert!(history[0].instance_id.is_some());
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

    #[test]
    fn purge_before_removes_expired_events_from_memory() {
        let db = temp_db();
        let cutoff = Utc::now();
        for (id, at) in [
            ("old", cutoff - chrono::Duration::seconds(1)),
            ("new", cutoff),
        ] {
            crate::db::activity::insert(
                &db,
                &ActivityEvent {
                    id: id.to_string(),
                    at,
                    game: GameId::Valheim,
                    instance: None,
                    instance_id: None,
                    kind: ActivityKind::InstanceCreated,
                },
            )
            .unwrap();
        }
        let log = ActivityLog::load(db);

        assert_eq!(log.purge_before(cutoff).unwrap(), 1);
        let (history, _rx) = log.subscribe();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "new");
    }

    #[test]
    fn failure_events_are_cleaned_and_capped() {
        let ActivityKind::JobFailed { message, .. } = ActivityKind::job_failed(
            "backup_create".to_string(),
            &format!(
                "upload to https://u:p@s3.example/b failed {}",
                "x".repeat(5000)
            ),
        ) else {
            panic!("not a job failure");
        };
        assert!(message.starts_with("upload to https://***@s3.example/b failed"));
        assert_eq!(message.chars().count(), MAX_FAILURE_TEXT);

        let ActivityKind::StartupFailed { reason } =
            ActivityKind::startup_failed("rcon password: hunter2 refused")
        else {
            panic!("not a startup failure");
        };
        assert_eq!(reason, "rcon password: *** refused");
    }
}
