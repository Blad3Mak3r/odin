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
//! Current Valheim logs report the SteamID and character name on separate
//! lines. `PlayerEventParser` correlates those lines so the later
//! `Closing socket <SteamID>` message can remove the right player.

use std::collections::{HashSet, VecDeque};
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

static LEGACY_ZDOID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Got character ZDOID from client (\d+)\s*:\s*(.+)$").unwrap());
static CONNECTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Got connection SteamID (\d+)").unwrap());
static ZDOID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Got character ZDOID from (.+?)\s*:\s*\d+:\d+").unwrap());
static DISCONNECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Closing socket (\d+)").unwrap());

#[derive(Default)]
pub struct PlayerEventParser {
    pending: VecDeque<String>,
    active: HashSet<String>,
}

impl PlayerEventParser {
    /// Recognizes join/leave events while retaining the connection identity
    /// needed to correlate Valheim's separate SteamID and character lines.
    pub fn parse_line(&mut self, line: &str) -> Option<PlayerEvent> {
        if let Some(caps) = LEGACY_ZDOID_RE.captures(line) {
            let peer = caps[1].to_string();
            self.pending.retain(|candidate| candidate != &peer);
            self.active.insert(peer.clone());
            return Some(PlayerEvent::Joined {
                peer,
                name: caps[2].trim().to_string(),
            });
        }
        if let Some(caps) = CONNECTION_RE.captures(line) {
            let peer = caps[1].to_string();
            if !self.active.contains(&peer) && !self.pending.contains(&peer) {
                self.pending.push_back(peer);
            }
        }
        if let Some(caps) = ZDOID_RE.captures(line) {
            let peer = self.pending.pop_front()?;
            self.active.insert(peer.clone());
            return Some(PlayerEvent::Joined {
                peer,
                name: caps[1].trim().to_string(),
            });
        }
        if let Some(caps) = DISCONNECT_RE.captures(line) {
            let peer = caps[1].to_string();
            self.pending.retain(|candidate| candidate != &peer);
            self.active.remove(&peer);
            return Some(PlayerEvent::Left { peer });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zdoid_line_is_a_join() {
        let event = PlayerEventParser::default()
            .parse_line("14:32:10: Got character ZDOID from client 0 : Bjorn");
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
        let event = PlayerEventParser::default().parse_line("14:40:02: Closing socket 0");
        assert_eq!(
            event,
            Some(PlayerEvent::Left {
                peer: "0".to_string(),
            })
        );
    }

    #[test]
    fn unrelated_line_is_ignored() {
        assert_eq!(
            PlayerEventParser::default().parse_line("14:32:00: World saved"),
            None
        );
    }

    #[test]
    fn current_log_format_correlates_name_with_steam_id() {
        let mut parser = PlayerEventParser::default();

        assert_eq!(
            parser.parse_line("08/31/2026 19:00:28: Got connection SteamID 76561198137061571"),
            None
        );
        assert_eq!(
            parser.parse_line("08/31/2026 19:00:28: Got connection SteamID 76561198137061571"),
            None
        );
        assert_eq!(
            parser
                .parse_line("08/31/2026 19:00:50: Got character ZDOID from Poucho : 1839760071:1"),
            Some(PlayerEvent::Joined {
                peer: "76561198137061571".to_string(),
                name: "Poucho".to_string(),
            })
        );
        assert_eq!(
            parser.parse_line("08/31/2026 19:04:03: Closing socket 76561198137061571"),
            Some(PlayerEvent::Left {
                peer: "76561198137061571".to_string(),
            })
        );
    }
}
