//! `odin serve`'s handle onto `crate::supervisor` (the `odin run` RPC
//! layer). `instance::lifecycle` already talks to it directly for
//! start/stop, and `web::routes::resources::compute_instance_snapshot`
//! pings it for liveness; this module bridges a running instance's pushed
//! log/exit events into `LogTailRegistry`, so `web::sse`'s live console
//! stream is fed by `odin run` pushing lines over its events socket rather
//! than `web::log_tail` polling `console.log` from here. `web::mod`'s
//! `reconcile_log_tailers` falls back to that file-poller only for an
//! instance with no reachable supervisor (see `try_bridge_events`).

use futures_util::StreamExt as _;
use tokio::task::AbortHandle;

use crate::activity::ActivityLog;
use crate::paths::Paths;
use crate::supervisor::client;
use crate::supervisor::protocol::Event;
use crate::web::log_tail::LogTailRegistry;
use crate::web::players::PlayerRegistry;

#[derive(Clone, Default)]
pub struct Supervisor;

impl Supervisor {
    pub fn new() -> Self {
        Self
    }

    /// Subscribes to `name`'s events socket and bridges pushed `LogLine`s
    /// into `log_tail`'s broadcast channel (applying player-tracking
    /// exactly like the legacy poller does), until the supervisor closes
    /// the connection — normally right after an `Exited` event. Returns
    /// `None` if no supervisor is reachable for this instance, so the
    /// caller can fall back to `web::log_tail`'s file-poller.
    pub async fn try_bridge_events(
        &self,
        paths: &Paths,
        name: &str,
        log_tail: &LogTailRegistry,
        players: &PlayerRegistry,
        activity: &ActivityLog,
    ) -> Option<AbortHandle> {
        let stream = client::subscribe_events(paths, name).await.ok()?;
        let sender = log_tail.sender_for(name);
        let name = name.to_string();
        let players = players.clone();
        let activity = activity.clone();

        let handle = tokio::spawn(async move {
            futures_util::pin_mut!(stream);
            while let Some(event) = stream.next().await {
                match event {
                    Event::LogLine { line } => {
                        if let Some(kind) = players.apply_line(&name, &line) {
                            activity.record(kind, Some(name.clone()));
                        }
                        let _ = sender.send(line);
                    }
                    Event::Exited { .. } => break,
                }
            }
        });
        Some(handle.abort_handle())
    }
}
