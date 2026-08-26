//! One shared tailer per running instance for `console.log`: a single
//! background task polls the file and broadcasts each new line to every
//! subscriber (live log-viewer WebSockets, the player-connection tracker),
//! instead of each subscriber polling the file on its own.
//!
//! `console.log` is never rotated or truncated, so unlike `web::jobs`'
//! `JobRegistry` this deliberately keeps no in-memory history of lines —
//! that would just move the same unbounded growth from disk into RAM. The
//! file itself (via `commands::logs::read_tail`) stays the source of truth
//! for a subscriber's initial backlog; the broadcast channel only ever
//! carries lines appended from the moment of subscription onward.

use std::collections::HashMap;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::broadcast;

use crate::activity::ActivityLog;
use crate::web::players::PlayerRegistry;

const BROADCAST_CAPACITY: usize = 256;
const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Default)]
pub struct LogTailRegistry {
    channels: Arc<Mutex<HashMap<String, broadcast::Sender<String>>>>,
}

impl LogTailRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the broadcast sender for `name`, creating one on first use.
    /// The sender exists independently of whether a tailer task is
    /// currently running for it, so a client can subscribe before an
    /// instance has ever started (or after it stopped) and simply receive
    /// nothing until `tail_and_broadcast` is spawned for it.
    pub fn sender_for(&self, name: &str) -> broadcast::Sender<String> {
        self.channels
            .lock()
            .expect("log tail registry lock poisoned")
            .entry(name.to_string())
            .or_insert_with(|| broadcast::channel(BROADCAST_CAPACITY).0)
            .clone()
    }
}

/// The single poll loop for a running instance's `console.log`: reads
/// newly-appended bytes, applies any player join/leave recognized in each
/// line, and broadcasts the line to every live subscriber. Runs until
/// aborted by the telemetry loop when the instance stops running (see
/// `web::mod`'s `reconcile_log_tailers`). Starts tailing from the file's
/// current end, not its beginning, since this only spawns once an instance
/// is observed running — replaying its whole history isn't useful and
/// could be a lot of lines for a long-running server.
pub async fn tail_and_broadcast(
    instance_name: String,
    log_file: PathBuf,
    sender: broadcast::Sender<String>,
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
            if let Some(kind) = players.apply_line(&instance_name, line) {
                activity.record(kind, Some(instance_name.clone()));
            }
            let _ = sender.send(line.to_string());
        }
    }
}

/// Reads whatever has been appended to `path` since byte offset `from`.
/// Restarts from the beginning if the file is now shorter than `from`
/// (rotated/truncated). Never errors — a transient read failure just yields
/// no new bytes this tick, and the next poll tries again.
fn read_new_bytes(path: &StdPath, from: u64) -> (u64, String) {
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
