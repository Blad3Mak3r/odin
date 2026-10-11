//! Recognizes Valheim's own log line for "finished loading and actually
//! accepting connections" — as opposed to just "the process exists", which
//! `supervisor::client::ping`/`ping_blocking` already tell you regardless of
//! how far the world has loaded. Used only by `supervisor::server` today
//! (there's no host-side equivalent for an instance with no reachable
//! supervisor, unlike `player_events`/`save_events` — readiness just isn't
//! observable that way), but kept as its own small top-level module for the
//! same reason those are: easy to isolate and fix if the real log output
//! doesn't match.

/// Best-effort, like `player_events::PlayerEventParser`/`save_events::
/// is_world_saved_line` — not verified against a real `console.log`. If it
/// doesn't match a real server's output, this is the one place to fix.
pub fn is_ready_line(game: crate::game::GameId, line: &str) -> bool {
    match game {
        crate::game::GameId::Valheim => line.contains("Game server connected"),
        // 7D2D writes these after the world has loaded and it starts accepting
        // remote connections. Keep this deliberately conservative: a process
        // merely binding its ports is not yet ready to host players.
        crate::game::GameId::SevenDaysToDie => {
            line.contains("Game started")
                || line.contains("GamePref.ServerIsRunning = true")
                || line.contains("Started game server")
        }
        crate::game::GameId::Rust
        | crate::game::GameId::VRising
        | crate::game::GameId::Palworld
        | crate::game::GameId::RunescapeDragonwilds => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_server_connected_line_is_recognized() {
        assert!(is_ready_line(
            crate::game::GameId::Valheim,
            "08/29 20:18:16: Game server connected"
        ));
    }

    #[test]
    fn unrelated_line_is_not_readiness() {
        assert!(!is_ready_line(
            crate::game::GameId::Valheim,
            "08/29 20:18:16: Registering lobby"
        ));
    }

    #[test]
    fn seven_days_start_line_is_recognized() {
        assert!(is_ready_line(
            crate::game::GameId::SevenDaysToDie,
            "INF Game started"
        ));
    }
}
