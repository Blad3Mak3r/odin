//! `odin serve`'s handle onto `crate::supervisor` (the `odin run` RPC
//! layer). `instance::lifecycle` already talks to it directly for
//! start/stop, and `web::routes::resources::compute_instance_snapshot`
//! pings it for liveness; this module bridges a running instance's pushed
//! log/player/save/exit events into `LogTailRegistry`/`PlayerRegistry`/
//! `WorldSaveRegistry`, so `web::sse`'s live console stream and the
//! players/last-saved views are fed by `odin run` pushing over its events
//! socket rather than `odin serve` polling `console.log` (for lines) or
//! re-parsing it (for players/saves) from here. `web::mod`'s
//! `reconcile_log_tailers` falls back to that file-poller only for an
//! instance with no reachable supervisor (see `try_bridge_events`).

use futures_util::StreamExt as _;
use tokio::task::AbortHandle;

use crate::activity::ActivityLog;
use crate::paths::Paths;
use crate::supervisor::client;
use crate::supervisor::protocol::{Event, Response};
use crate::web::log_tail::LogTailRegistry;
use crate::web::players::PlayerRegistry;
use crate::web::world_saves::WorldSaveRegistry;

#[derive(Clone, Default)]
pub struct Supervisor;

impl Supervisor {
    pub fn new() -> Self {
        Self
    }

    /// Subscribes to `name`'s events socket and bridges pushed events into
    /// `log_tail`'s broadcast channel (`LogLine`), `players`
    /// (`PlayerJoined`/`PlayerLeft`), and `world_saves` (`WorldSaved`) —
    /// all already structured by the supervisor's own parsing, no
    /// re-parsing needed here — until the supervisor closes the connection,
    /// normally right after an `Exited` event. Seeds `players`/
    /// `world_saves` with the supervisor's current state first (via
    /// `client::players`/`client::last_saved`), so anything that happened
    /// before this subscription started (e.g. `odin serve` just restarted)
    /// isn't missed. Returns `None` if no supervisor is reachable for this
    /// instance, so the caller can fall back to `web::log_tail`'s
    /// file-poller.
    pub async fn try_bridge_events(
        &self,
        paths: &Paths,
        name: &str,
        log_tail: &LogTailRegistry,
        players: &PlayerRegistry,
        world_saves: &WorldSaveRegistry,
        activity: &ActivityLog,
    ) -> Option<AbortHandle> {
        let stream = client::subscribe_events(paths, name).await.ok()?;

        if let Ok(Response::Players {
            players: current_players,
        }) = client::players(paths, name).await
        {
            players.replace_snapshot(name, current_players);
        }
        if let Ok(Response::LastSaved { at: Some(at) }) = client::last_saved(paths, name).await {
            world_saves.set(name, at);
        }

        let sender = log_tail.sender_for(name);
        let name = name.to_string();
        let players = players.clone();
        let world_saves = world_saves.clone();
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
                    Event::WorldSaved { at } => {
                        world_saves.set(&name, at);
                    }
                    Event::Exited { .. } => break,
                }
            }
        });
        Some(handle.abort_handle())
    }
}
