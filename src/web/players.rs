//! Tracks which players are currently connected to each running instance by
//! tailing its `console.log` and recognizing Valheim's own connection
//! messages — the dedicated server has no RCON or other admin protocol, so
//! this is the only signal available.
//!
//! The patterns below are a best-effort reconstruction from how Valheim's
//! `ZNet`/`ZDOID` logging is known to behave (a numeric peer id shows up in
//! both the join and the disconnect line), not verified against a real
//! `console.log`. If they don't match a real server's output, `parse_line`
//! is the one place to fix: it's a pure function, easy to test/adjust in
//! isolation without touching the tailer or the registry around it. An
//! unmatched line — including a disconnect whose peer id isn't currently
//! tracked — is silently ignored rather than guessed at, so a bad pattern
//! degrades to "missing a leave event" rather than corrupting the list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Serialize;

use crate::activity::{ActivityKind, ActivityLog};
use crate::web::ws::read_new_bytes;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize)]
pub struct PlayerInfo {
    pub name: String,
    pub connected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlayerEvent {
    Joined { peer: String, name: String },
    Left { peer: String },
}

static ZDOID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Got character ZDOID from client (\d+)\s*:\s*(.+)$").unwrap());
static DISCONNECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Closing socket (\d+)").unwrap());

/// Recognizes a join or leave in a single `console.log` line. See the
/// module doc comment — this is the best-effort, adjustable part.
fn parse_line(line: &str) -> Option<PlayerEvent> {
    if let Some(caps) = ZDOID_RE.captures(line) {
        return Some(PlayerEvent::Joined {
            peer: caps[1].to_string(),
            name: caps[2].trim().to_string(),
        });
    }
    if let Some(caps) = DISCONNECT_RE.captures(line) {
        return Some(PlayerEvent::Left {
            peer: caps[1].to_string(),
        });
    }
    None
}

struct ConnectedPlayer {
    peer: String,
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

    fn apply(&self, instance: &str, event: PlayerEvent) -> Option<ActivityKind> {
        let mut instances = self
            .instances
            .lock()
            .expect("players registry lock poisoned");
        let players = instances.entry(instance.to_string()).or_default();
        match event {
            PlayerEvent::Joined { peer, name } => {
                if players.iter().any(|p| p.peer == peer) {
                    return None;
                }
                players.push(ConnectedPlayer {
                    peer,
                    info: PlayerInfo {
                        name: name.clone(),
                        connected_at: Utc::now(),
                    },
                });
                Some(ActivityKind::PlayerJoined { name })
            }
            PlayerEvent::Left { peer } => {
                let index = players.iter().position(|p| p.peer == peer)?;
                let removed = players.remove(index);
                Some(ActivityKind::PlayerLeft {
                    name: removed.info.name,
                })
            }
        }
    }
}

impl Default for PlayerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Polls `log_file` for newly-appended lines (same mechanism as the
/// console WebSocket's tail-follow) and applies any join/leave recognized
/// in them, until aborted by the telemetry supervisor. Starts tailing from
/// the file's current end, not its beginning, since this only spawns once
/// an instance is observed running — replaying its whole history isn't
/// useful and could be a lot of lines for a long-running server.
pub async fn tail_console_log(
    instance_name: String,
    log_file: PathBuf,
    players: PlayerRegistry,
    activity: ActivityLog,
) {
    let mut pos = tokio::task::spawn_blocking({
        let log_file = log_file.clone();
        move || std::fs::metadata(&log_file).map(|m| m.len()).unwrap_or(0)
    })
    .await
    .unwrap_or(0);

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;

        let file = log_file.clone();
        let (new_pos, chunk) = tokio::task::spawn_blocking(move || read_new_bytes(&file, pos))
            .await
            .unwrap_or((pos, String::new()));
        pos = new_pos;
        if chunk.is_empty() {
            continue;
        }

        for line in chunk.lines() {
            let Some(event) = parse_line(line) else {
                continue;
            };
            if let Some(kind) = players.apply(&instance_name, event) {
                activity.record(kind, Some(instance_name.clone()));
            }
        }
    }
}

pub fn console_log_path(instance_dir: &Path) -> PathBuf {
    crate::paths::instance_logs_dir(instance_dir).join("console.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zdoid_line_is_a_join() {
        let event = parse_line("14:32:10: Got character ZDOID from client 0 : Bjorn");
        assert_eq!(
            event,
            Some(PlayerEvent::Joined {
                peer: "0".to_string(),
                name: "Bjorn".to_string(),
            })
        );
    }

    #[test]
    fn closing_socket_line_is_a_leave() {
        let event = parse_line("14:40:02: Closing socket 0");
        assert_eq!(
            event,
            Some(PlayerEvent::Left {
                peer: "0".to_string(),
            })
        );
    }

    #[test]
    fn unrelated_line_is_ignored() {
        assert_eq!(parse_line("14:32:00: World saved"), None);
    }

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
}
