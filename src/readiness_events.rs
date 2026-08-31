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
pub fn is_ready_line(line: &str) -> bool {
    line.contains("Game server connected")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_server_connected_line_is_recognized() {
        assert!(is_ready_line("08/29 20:18:16: Game server connected"));
    }

    #[test]
    fn unrelated_line_is_not_readiness() {
        assert!(!is_ready_line("08/29 20:18:16: Registering lobby"));
    }
}
