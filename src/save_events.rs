//! Recognizes Valheim's own "world saved" `console.log` line — shared by
//! `supervisor::server` (parses lines as it tails them, for `odin run`'s
//! own self-monitoring) and `web::log_tail` (`odin serve`'s fallback for an
//! instance with no reachable supervisor), same reasoning as
//! `crate::player_events`.

/// Best-effort, like `player_events::PlayerEventParser` — not verified against a
/// real `console.log`. If it doesn't match a real server's output, this is
/// the one place to fix.
pub fn is_world_saved_line(line: &str) -> bool {
    line.contains("World saved")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_saved_line_is_recognized() {
        assert!(is_world_saved_line("14:32:00: World saved"));
    }

    #[test]
    fn unrelated_line_is_not_a_save() {
        assert!(!is_world_saved_line(
            "14:32:10: Got character ZDOID from client 0 : Bjorn"
        ));
    }
}
