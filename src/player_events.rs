//! Recognizes Valheim's own connection messages in `console.log` lines —
//! the dedicated server has no RCON or other admin protocol, so this is the
//! only signal available for who's currently connected.
//!
//! Shared by `supervisor::server` (which parses lines as it tails them, for
//! `odin run`'s own player self-monitoring) and `web::players`/
//! `web::log_tail` (`odin serve`'s fallback for an instance with no
//! reachable supervisor) — neither owns the other, so this pure logic lives
//! at the crate root, the same way `log_poll` does for raw line-reading.
//!
//! The patterns below are a best-effort reconstruction from how Valheim's
//! `ZNet`/`ZDOID` logging is known to behave (a numeric peer id shows up in
//! both the join and the disconnect line), not verified against a real
//! `console.log`. If they don't match a real server's output, `parse_line`
//! is the one place to fix: it's a pure function, easy to test/adjust in
//! isolation without touching either side that consumes it. An unmatched
//! line — including a disconnect whose peer id isn't currently tracked — is
//! silently ignored rather than guessed at, so a bad pattern degrades to
//! "missing a leave event" rather than corrupting the list.

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub name: String,
    pub connected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerEvent {
    Joined { peer: String, name: String },
    Left { peer: String },
}

static ZDOID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Got character ZDOID from client (\d+)\s*:\s*(.+)$").unwrap());
static DISCONNECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Closing socket (\d+)").unwrap());

/// Recognizes a join or leave in a single `console.log` line. See the
/// module doc comment — this is the best-effort, adjustable part.
pub fn parse_line(line: &str) -> Option<PlayerEvent> {
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
}
