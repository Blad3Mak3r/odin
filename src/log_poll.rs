//! Shared byte-offset file-tailing helper. Used by both `web::log_tail`'s
//! per-boot poller (the legacy fallback for instances with no reachable
//! supervisor) and `supervisor::server`'s in-process poller.

use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;

/// Reads whatever has been appended to `path` since byte offset `from`.
/// Restarts from the beginning if the file is now shorter than `from`
/// (rotated/truncated). Never errors — a transient read failure just yields
/// no new bytes this tick, and the next poll tries again.
pub fn read_new_bytes(path: &Path, from: u64) -> (u64, String) {
    let Ok(mut file) = std::fs::File::open(path) else {
        return (from, String::new());
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(from);
    let start = if len < from { 0 } else { from };
    if file.seek(SeekFrom::Start(start)).is_err() {
        return (len, String::new());
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return (len, String::new());
    }
    (len, String::from_utf8_lossy(&buf).to_string())
}
