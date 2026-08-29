//! `odin serve`'s handle onto `crate::supervisor` (the `odin run` RPC
//! layer). `instance::lifecycle` already talks to it directly for
//! start/stop, and `web::routes::resources::compute_instance_snapshot`
//! pings it for liveness; this module bridges a running instance's pushed
//! log/player/exit events into `LogTailRegistry`/`PlayerRegistry`, so
//! `web::sse`'s live console stream and the players list are fed by `odin
//! run` pushing over its events socket rather than `odin serve` polling
//! `console.log` (for lines) or re-parsing it (for players) from here.
//! `web::mod`'s `reconcile_log_tailers` falls back to that file-poller only
//! for an instance with no reachable supervisor (see `try_bridge_events`).

use futures_util::StreamExt as _;
use tokio::task::AbortHandle;

use crate::activity::ActivityLog;
use crate::paths::Paths;
use crate::supervisor::client;
use crate::supervisor::protocol::{Event, Response};
use crate::web::log_tail::LogTailRegistry;
use crate::web::players::PlayerRegistry;

#[derive(Clone, Default)]
pub struct Supervisor;

impl Supervisor {
    pub fn new() -> Self {
        Self
    }

    /// Subscribes to `name`'s events socket and bridges pushed events into
    /// `log_tail`'s broadcast channel (`LogLine`) and `players`
    /// (`PlayerJoined`/`PlayerLeft`, already structured by the supervisor's
    /// own parsing — no re-parsing needed here), until the supervisor
    /// closes the connection — normally right after an `Exited` event.
    /// Seeds `players` with the supervisor's current list first (via
    /// `client::players`), so anyone already connected before this
    /// subscription started (e.g. `odin serve` just restarted) isn't
    /// missed. Returns `None` if no supervisor is reachable for this
    /// instance, so the caller can fall back to `web::log_tail`'s
    /// file-poller.
    pub async fn try_bridge_events(
        &self,
        paths: &Paths,
        name: &str,
        log_tail: &LogTailRegistry,
        players: &PlayerRegistry,
        activity: &ActivityLog,
    ) -> Option<AbortHandle> {
        let stream = client::subscribe_events(paths, name).await.ok()?;

        if let Ok(Response::Players {
            players: current_players,
        }) = client::players(paths, name).await
        {
            players.replace_snapshot(name, current_players);
        }

        let sender = log_tail.sender_for(name);
        let name = name.to_string();
        let players = players.clone();
        let activity = activity.clone();

        let handle = tokio::spawn(async move {
            futures_util::pin_mut!(stream);
            while let Some(event) = stream.next().await {
                match event {
                    Event::LogLine { line } => {
                        let _ = sender.send(line);
                    }
                    Event::PlayerJoined { name: player } => {
                        if let Some(kind) = players.mark_joined(&name, player) {
                            activity.record(kind, Some(name.clone()));
                        }
                    }
                    Event::PlayerLeft { name: player } => {
                        if let Some(kind) = players.mark_left(&name, &player) {
                            activity.record(kind, Some(name.clone()));
                        }
                    }
                    Event::Restarted => {
                        activity.record(
                            crate::activity::ActivityKind::InstanceAutoRestarted,
                            Some(name.clone()),
                        );
                    }
                    Event::Exited { .. } => break,
                }
            }
        });
        Some(handle.abort_handle())
    }
}
