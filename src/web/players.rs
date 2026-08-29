//! Tracks which players are currently connected to each running instance.
//!
//! Two sources feed this registry: a reachable instance's supervisor
//! (`odin run`) does its own `console.log` parsing and pushes structured
//! join/leave events (`web::supervisor::Supervisor::try_bridge_events`,
//! via `mark_joined`/`mark_left`/`replace_snapshot` below); an instance
//! with no reachable supervisor falls back to `web::log_tail` parsing raw
//! lines locally (`apply_line`, unchanged) — see `crate::player_events` for
//! the shared parsing logic itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;

use crate::activity::ActivityKind;
use crate::player_events::{PlayerEvent, parse_line};

pub use crate::player_events::PlayerInfo;

struct ConnectedPlayer {
    peer: Option<String>,
    info: PlayerInfo,
}

#[derive(Clone)]
pub struct PlayerRegistry {
    instances: Arc<Mutex<HashMap<String, Vec<ConnectedPlayer>>>>,
}

impl PlayerRegistry {
    pub fn new() -> Self {
        Self {
            instances: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn snapshot(&self, instance: &str) -> Vec<PlayerInfo> {
        self.instances
            .lock()
            .expect("players registry lock poisoned")
            .get(instance)
            .map(|players| players.iter().map(|p| p.info.clone()).collect())
            .unwrap_or_default()
    }

    /// Drops all tracked players for an instance whose tailer just stopped
    /// (it stopped running, or was deleted) — otherwise a crashed server
    /// would show its last-known player list as still connected forever.
    pub fn clear_instance(&self, instance: &str) {
        self.instances
            .lock()
            .expect("players registry lock poisoned")
            .remove(instance);
    }

    /// Recognizes a join/leave in a single `console.log` line (see
    /// `crate::player_events::parse_line`) and applies it, returning the
    /// activity to record if the line mattered. Used by `web::log_tail`'s
    /// fallback poller, for an instance with no reachable supervisor to
    /// push structured events instead.
    pub fn apply_line(&self, instance: &str, line: &str) -> Option<ActivityKind> {
        let event = parse_line(line)?;
        self.apply(instance, event)
    }

    fn apply(&self, instance: &str, event: PlayerEvent) -> Option<ActivityKind> {
        let mut instances = self
            .instances
            .lock()
            .expect("players registry lock poisoned");
        let players = instances.entry(instance.to_string()).or_default();
        match event {
            PlayerEvent::Joined { peer, name } => {
                if players.iter().any(|p| p.peer.as_deref() == Some(&*peer)) {
                    return None;
                }
                players.push(ConnectedPlayer {
                    peer: Some(peer),
                    info: PlayerInfo {
                        name: name.clone(),
                        connected_at: Utc::now(),
                    },
                });
                Some(ActivityKind::PlayerJoined { name })
            }
            PlayerEvent::Left { peer } => {
                let index = players
                    .iter()
                    .position(|p| p.peer.as_deref() == Some(&*peer))?;
                let removed = players.remove(index);
                Some(ActivityKind::PlayerLeft {
                    name: removed.info.name,
                })
            }
        }
    }

    /// Records a join pushed by a reachable supervisor as a structured
    /// `Event::PlayerJoined` (see `web::supervisor::Supervisor::
    /// try_bridge_events`) — the supervisor already resolved the peer id
    /// internally, so this is keyed by name, deduplicated the same way
    /// `apply`'s peer-keyed path is.
    pub fn mark_joined(&self, instance: &str, name: String) -> Option<ActivityKind> {
        let mut instances = self
            .instances
            .lock()
            .expect("players registry lock poisoned");
        let players = instances.entry(instance.to_string()).or_default();
        if players
            .iter()
            .any(|p| p.peer.is_none() && p.info.name == name)
        {
            return None;
        }
        players.push(ConnectedPlayer {
            peer: None,
            info: PlayerInfo {
                name: name.clone(),
                connected_at: Utc::now(),
            },
        });
        Some(ActivityKind::PlayerJoined { name })
    }

    /// The `Event::PlayerLeft` counterpart to `mark_joined`.
    pub fn mark_left(&self, instance: &str, name: &str) -> Option<ActivityKind> {
        let mut instances = self
            .instances
            .lock()
            .expect("players registry lock poisoned");
        let players = instances.entry(instance.to_string()).or_default();
        let index = players
            .iter()
            .position(|p| p.peer.is_none() && p.info.name == name)?;
        let removed = players.remove(index);
        Some(ActivityKind::PlayerLeft {
            name: removed.info.name,
        })
    }

    /// Seeds `instance`'s current player list from a supervisor's
    /// authoritative `Response::Players` snapshot, replacing whatever was
    /// tracked before — used right after a bridge connects, to pick up
    /// anyone who joined before this subscription started (e.g. `odin
    /// serve` restarted while the game kept running).
    pub fn replace_snapshot(&self, instance: &str, players: Vec<PlayerInfo>) {
        self.instances
            .lock()
            .expect("players registry lock poisoned")
            .insert(
                instance.to_string(),
                players
                    .into_iter()
                    .map(|info| ConnectedPlayer { peer: None, info })
                    .collect(),
            );
    }
}

impl Default for PlayerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn console_log_path(instance_dir: &Path) -> PathBuf {
    crate::paths::instance_logs_dir(instance_dir).join("console.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_then_leave_round_trips_through_the_registry() {
        let registry = PlayerRegistry::new();

        let joined = registry.apply(
            "my-server",
            PlayerEvent::Joined {
                peer: "0".to_string(),
                name: "Bjorn".to_string(),
            },
        );
        assert!(matches!(joined, Some(ActivityKind::PlayerJoined { name }) if name == "Bjorn"));
        assert_eq!(registry.snapshot("my-server").len(), 1);

        let left = registry.apply(
            "my-server",
            PlayerEvent::Left {
                peer: "0".to_string(),
            },
        );
        assert!(matches!(left, Some(ActivityKind::PlayerLeft { name }) if name == "Bjorn"));
        assert_eq!(registry.snapshot("my-server").len(), 0);
    }

    #[test]
    fn leave_with_unknown_peer_is_a_noop() {
        let registry = PlayerRegistry::new();
        let result = registry.apply(
            "my-server",
            PlayerEvent::Left {
                peer: "99".to_string(),
            },
        );
        assert!(result.is_none());
    }

    #[test]
    fn mark_joined_then_mark_left_round_trips() {
        let registry = PlayerRegistry::new();

        let joined = registry.mark_joined("my-server", "Bjorn".to_string());
        assert!(matches!(joined, Some(ActivityKind::PlayerJoined { name }) if name == "Bjorn"));
        assert_eq!(registry.snapshot("my-server").len(), 1);

        // A duplicate join for the same name is a no-op, same as apply's
        // peer-keyed dedup.
        assert!(
            registry
                .mark_joined("my-server", "Bjorn".to_string())
                .is_none()
        );
        assert_eq!(registry.snapshot("my-server").len(), 1);

        let left = registry.mark_left("my-server", "Bjorn");
        assert!(matches!(left, Some(ActivityKind::PlayerLeft { name }) if name == "Bjorn"));
        assert_eq!(registry.snapshot("my-server").len(), 0);
    }

    #[test]
    fn replace_snapshot_seeds_the_current_list() {
        let registry = PlayerRegistry::new();
        registry.apply(
            "my-server",
            PlayerEvent::Joined {
                peer: "0".to_string(),
                name: "Stale".to_string(),
            },
        );

        registry.replace_snapshot(
            "my-server",
            vec![PlayerInfo {
                name: "Bjorn".to_string(),
                connected_at: Utc::now(),
            }],
        );

        let snapshot = registry.snapshot("my-server");
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].name, "Bjorn");
    }
}
