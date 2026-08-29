//! The `odin run` side of the supervisor: the conmon-equivalent role. Binds
//! an instance's `control.sock`/`events.sock`, launches its Valheim process
//! via `instance::process` (reused verbatim — this is the only code path
//! that calls it once `instance::lifecycle` is wired to use this module),
//! and owns the resulting `Child` for this process's entire lifetime.
//!
//! Signalling/stopping the child no longer needs the pid-fingerprint dance
//! `instance::process::send_signal` exists for: this process holds the
//! actual `tokio::process::Child`, so `child.wait()` reaps it directly and
//! races with pid reuse are structurally impossible. The fingerprint-based
//! helpers are still used for the initial signal delivery itself (SIGINT/
//! SIGKILL take a pid, not a `Child`) but liveness after that is just
//! "has `child.wait()` resolved yet".

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use sysinfo::{Pid, Signal, System};
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};

use super::protocol::{
    ErrorCode, Event, PROTOCOL_VERSION, Request, Response, read_frame, write_frame,
};
use crate::db::Db;
use crate::instance::{Instance, lifecycle, process};
use crate::paths::{self, Paths};

const EVENT_BROADCAST_CAPACITY: usize = 256;
const CONTROL_READ_TIMEOUT: Duration = Duration::from_secs(5);
const LOG_POLL_INTERVAL: Duration = Duration::from_millis(500);
// Matches `web::TELEMETRY_INTERVAL` in value only — independently owned,
// since this tick loop belongs to a different process (`odin run`, not
// `odin serve`) and can evolve on its own.
const STATS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
// Matches `web::AUTO_RESTART_COOLDOWN` in value only — independently owned
// (see that constant's doc comment for the rationale: without a cooldown,
// an instance that crashes immediately on every start would get a fresh
// attempt every time `child.wait()` resolves). `odin serve`'s own copy of
// this cooldown still applies too, for the instance-has-no-live-supervisor
// fallback path — this one is strictly faster since the supervisor learns
// about the exit instantly instead of on the next ~3s telemetry poll.
const RESTART_COOLDOWN: Duration = Duration::from_secs(60);
// How many trailing `console.log` lines `Request::LastExit` carries —
// enough to see what led up to a crash without turning every exit
// diagnostic into a full log dump.
pub(crate) const RECENT_LINES_CAPACITY: usize = 20;

/// Shared with `handle_control_connection`: `None` until the first
/// background refresh completes, then the latest `(cpu_percent,
/// memory_bytes)` snapshot.
type StatsHandle = Arc<Mutex<Option<(f32, u64)>>>;

/// Shared with `handle_control_connection`: the currently-connected player
/// list, as recognized by `poll_console_log`'s own parsing. Peer ids are
/// only meaningful at this source (the supervisor resolves them; consumers
/// only ever see names), so they're kept here rather than exposed further.
type PlayersHandle = Arc<Mutex<Vec<TrackedPlayer>>>;

struct TrackedPlayer {
    peer: String,
    info: crate::player_events::PlayerInfo,
}

/// Shared with `handle_control_connection`: when the world was last saved,
/// as recognized by `poll_console_log`'s own parsing (`crate::save_events::
/// is_world_saved_line`). `None` until the first save this supervisor has
/// seen.
type SavedHandle = Arc<Mutex<Option<chrono::DateTime<Utc>>>>;

/// Shared with `handle_control_connection`: whether Valheim has finished
/// loading and is actually accepting connections, as recognized by
/// `poll_console_log`'s own parsing (`crate::readiness_events::
/// is_ready_line`) — `false` from spawn (or respawn) until then.
type ReadyHandle = Arc<Mutex<bool>>;

/// Shared with `handle_control_connection`: a bounded tail of the most
/// recent `console.log` lines, oldest first — snapshotted into
/// `LastExitHandle` whenever the child exits, so `Request::LastExit`
/// carries the context leading up to it.
type RecentLinesHandle = Arc<Mutex<std::collections::VecDeque<String>>>;

/// Shared with `handle_control_connection`: diagnostics for the most
/// recent exit of this supervisor's child — `None` until the first exit.
type LastExitHandle = Arc<Mutex<Option<super::protocol::LastExitInfo>>>;

/// Bundles the self-monitoring state `handle_control_connection` reads
/// from, so spawning it per connection doesn't need one parameter per
/// tracker (clippy's `too_many_arguments`) — cheap to clone, since every
/// field is itself just an `Arc`.
#[derive(Clone)]
struct SharedHandles {
    stats: StatsHandle,
    players: PlayersHandle,
    last_saved: SavedHandle,
    ready: ReadyHandle,
    last_exit: LastExitHandle,
}

/// A freshly spawned (or respawned, on automatic restart) child and its
/// identity fingerprint — everything `run_instance` needs to update its
/// local state and the DB, whether this is the initial spawn or a later
/// in-place restart.
struct SpawnedChild {
    child: tokio::process::Child,
    pid: u32,
    pid_started_at: i64,
    started_at: chrono::DateTime<Utc>,
}

/// Builds and spawns `instance`'s Valheim process, reading its own kernel
/// start time right away for the liveness fingerprint. Shared by
/// `run_instance`'s initial spawn and its automatic-restart path so both
/// go through identical setup.
async fn spawn_child(
    instance: &Instance,
    paths: &Paths,
    instance_name: &str,
) -> Result<SpawnedChild> {
    let cmd = process::build_command(instance, paths)?;
    let child = process::spawn(cmd)
        .await
        .with_context(|| format!("failed to start instance '{instance_name}'"))?;
    let pid = child.id().context("spawned child has no pid")?;
    let pid_started_at = process::start_time_of(pid)?;
    let started_at = Utc::now();
    Ok(SpawnedChild {
        child,
        pid,
        pid_started_at,
        started_at,
    })
}

/// Runs the supervisor for `instance_name` end to end: prepares and spawns
/// the Valheim process, serves the control/events sockets, and blocks until
/// the process exits for good (either on its own with no automatic restart
/// eligible, or via a `Stop` request) and cleanup is complete. An exit that
/// *is* eligible for automatic restart (see `RESTART_COOLDOWN`) respawns
/// the child in place instead of returning. `commands::run::run` just calls
/// this and returns.
pub async fn run_instance(paths: Paths, instance_name: &str) -> Result<()> {
    let db = Db::open(&paths).context("failed to open database")?;
    let mut instance = lifecycle::prepare_start(&paths, &db, instance_name)?;

    let run_dir = paths.runtime_dir();
    std::fs::create_dir_all(&run_dir)
        .with_context(|| format!("failed to create {}", run_dir.display()))?;
    let control_path = super::control_sock_path(&paths, instance_name);
    let events_path = super::events_sock_path(&paths, instance_name);
    let pidfile = super::pidfile_path(&paths, instance_name);

    let control_listener = bind_private(&control_path)?;
    let events_listener = match bind_private(&events_path) {
        Ok(listener) => listener,
        Err(error) => {
            let _ = std::fs::remove_file(&control_path);
            return Err(error);
        }
    };
    write_pidfile(&pidfile)?;

    let spawned = spawn_child(&instance, &paths, instance_name).await?;
    let mut child = spawned.child;
    let mut pid = spawned.pid;
    let mut pid_started_at = spawned.pid_started_at;
    let mut started_at = spawned.started_at;
    instance.state.pid = Some(pid);
    instance.state.pid_started_at = Some(pid_started_at);
    crate::db::instances::set_pid(&db, instance_name, pid, pid_started_at, started_at)?;

    let (events_tx, _) = broadcast::channel::<Event>(EVENT_BROADCAST_CAPACITY);
    let console_log = paths::instance_logs_dir(&instance.dir).join("console.log");
    let players: PlayersHandle = Arc::new(Mutex::new(Vec::new()));
    let last_saved: SavedHandle = Arc::new(Mutex::new(None));
    let ready: ReadyHandle = Arc::new(Mutex::new(false));
    let recent_lines: RecentLinesHandle = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    let last_exit: LastExitHandle = Arc::new(Mutex::new(None));
    let log_poller = tokio::spawn(poll_console_log(
        console_log,
        events_tx.clone(),
        players.clone(),
        last_saved.clone(),
        ready.clone(),
        recent_lines.clone(),
    ));

    let stats: StatsHandle = Arc::new(Mutex::new(None));
    let mut stats_refresher = tokio::spawn(refresh_stats_periodically(pid, stats.clone()));
    let handles = SharedHandles {
        stats: stats.clone(),
        players: players.clone(),
        last_saved: last_saved.clone(),
        ready: ready.clone(),
        last_exit: last_exit.clone(),
    };

    let (stop_tx, mut stop_rx) = mpsc::channel::<u64>(1);
    // Set once a `Stop` request starts the shutdown sequence below, so the
    // `child.wait()` arm that then fires can tell "asked to stop" apart
    // from "exited on its own" — only the latter is ever eligible for
    // automatic restart.
    let mut stopping = false;
    let mut last_restart_attempt: Option<tokio::time::Instant> = None;

    let exit_code = loop {
        tokio::select! {
            status = child.wait() => {
                let code = status.ok().and_then(|s| s.code());
                *last_exit.lock().expect("last exit lock poisoned") = Some(super::protocol::LastExitInfo {
                    code,
                    at: Utc::now(),
                    recent_lines: recent_lines.lock().expect("recent lines lock poisoned").iter().cloned().collect(),
                });
                if stopping {
                    break code;
                }

                // Re-read from the DB rather than trusting the `instance`
                // snapshot loaded at startup — an operator can toggle
                // auto-restart from the dashboard while this instance is
                // running.
                let auto_restart = crate::db::instances::load(&db, instance_name)
                    .ok()
                    .flatten()
                    .is_some_and(|s| s.auto_restart);
                let cooldown_elapsed = last_restart_attempt
                    .is_none_or(|at| at.elapsed() >= RESTART_COOLDOWN);

                if !auto_restart || !cooldown_elapsed {
                    break code;
                }
                last_restart_attempt = Some(tokio::time::Instant::now());

                tracing::warn!(
                    instance = instance_name,
                    ?code,
                    "instance exited unexpectedly; attempting automatic restart"
                );
                match spawn_child(&instance, &paths, instance_name).await {
                    Ok(respawned) => {
                        child = respawned.child;
                        pid = respawned.pid;
                        pid_started_at = respawned.pid_started_at;
                        started_at = respawned.started_at;
                        if let Err(e) = crate::db::instances::set_pid(
                            &db,
                            instance_name,
                            pid,
                            pid_started_at,
                            started_at,
                        ) {
                            tracing::warn!(instance = instance_name, error = %e, "failed to persist restarted pid");
                        }

                        // The old refresher is still tracking the dead
                        // pid; abort it before resetting the shared slot so
                        // nothing else can observe the stale value racing
                        // with the reset.
                        stats_refresher.abort();
                        *stats.lock().expect("stats lock poisoned") = None;
                        stats_refresher = tokio::spawn(refresh_stats_periodically(pid, stats.clone()));

                        // The new child hasn't reached readiness yet either.
                        *ready.lock().expect("ready lock poisoned") = false;

                        let _ = events_tx.send(Event::Restarted);
                    }
                    Err(e) => {
                        tracing::warn!(instance = instance_name, error = %e, "automatic restart failed");
                        break code;
                    }
                }
            }
            Ok((stream, _)) = control_listener.accept() => {
                tokio::spawn(handle_control_connection(
                    stream,
                    pid,
                    pid_started_at,
                    started_at,
                    stop_tx.clone(),
                    handles.clone(),
                ));
            }
            Ok((stream, _)) = events_listener.accept() => {
                tokio::spawn(handle_events_connection(stream, events_tx.subscribe()));
            }
            Some(timeout_secs) = stop_rx.recv() => {
                stopping = true;
                let _ = process::send_signal(pid, pid_started_at, Signal::Interrupt);
                if !process::wait_until_gone(pid, pid_started_at, Duration::from_secs(timeout_secs)).await {
                    tracing::warn!(
                        instance = instance_name,
                        "graceful shutdown did not complete within {timeout_secs}s; sending SIGKILL"
                    );
                    let _ = process::send_signal(pid, pid_started_at, Signal::Kill);
                }
                // Loop back around: `child.wait()` now resolves immediately
                // since the process has already exited.
            }
        }
    };

    log_poller.abort();
    stats_refresher.abort();
    let _ = events_tx.send(Event::Exited { code: exit_code });
    crate::db::instances::clear_pid(&db, instance_name, Utc::now())?;
    cleanup(&control_path, &events_path, &pidfile);

    tracing::info!(instance = instance_name, ?exit_code, "supervisor exiting");
    Ok(())
}

fn cleanup(control_path: &Path, events_path: &Path, pidfile: &Path) {
    let _ = std::fs::remove_file(control_path);
    let _ = std::fs::remove_file(events_path);
    let _ = std::fs::remove_file(pidfile);
}

/// Binds a Unix socket at `path`, reclaiming a stale socket file but refusing
/// to replace a live listener. The bind itself is the per-instance exclusion
/// point: concurrent supervisors cannot both own the control socket.
fn bind_private(path: &Path) -> Result<UnixListener> {
    let listener = loop {
        match UnixListener::bind(path) {
            Ok(listener) => break listener,
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                match std::os::unix::net::UnixStream::connect(path) {
                    Ok(_) => {
                        anyhow::bail!("supervisor socket is already in use: {}", path.display())
                    }
                    Err(_) => match std::fs::remove_file(path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(error) => {
                            return Err(error).with_context(|| {
                                format!("failed to remove stale socket {}", path.display())
                            });
                        }
                    },
                }
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to bind {}", path.display()));
            }
        }
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    Ok(listener)
}

fn write_pidfile(path: &Path) -> Result<()> {
    std::fs::write(path, std::process::id().to_string())
        .with_context(|| format!("failed to write {}", path.display()))
}

async fn handle_control_connection(
    stream: UnixStream,
    pid: u32,
    pid_started_at: i64,
    started_at: chrono::DateTime<Utc>,
    stop_tx: mpsc::Sender<u64>,
    handles: SharedHandles,
) {
    let SharedHandles {
        stats,
        players,
        last_saved,
        ready,
        last_exit,
    } = handles;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let request =
        match tokio::time::timeout(CONTROL_READ_TIMEOUT, read_frame::<Request, _>(&mut reader))
            .await
        {
            Ok(Ok(Some(request))) => request,
            Ok(Ok(None)) => return,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "failed to read control request");
                return;
            }
            Err(_) => {
                tracing::warn!("timed out waiting for control request");
                return;
            }
        };

    let response = match request {
        Request::Ping => Response::Pong {
            pid,
            pid_started_at,
            started_at,
            protocol_version: PROTOCOL_VERSION,
            odin_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ready: *ready.lock().expect("ready lock poisoned"),
        },
        Request::Stop { timeout_secs } => {
            // Best-effort: if the main loop already stopped listening (e.g.
            // the process is exiting for an unrelated reason), the send
            // fails silently — the caller will simply see the connection
            // close without a response, indistinguishable from "already
            // gone".
            let _ = stop_tx.send(timeout_secs).await;
            Response::Stopped
        }
        Request::Stats => match *stats.lock().expect("stats lock poisoned") {
            Some((cpu_percent, memory_bytes)) => Response::Stats {
                cpu_percent,
                memory_bytes,
            },
            None => Response::Error {
                code: ErrorCode::StatsNotReady,
                message: "stats not yet available".to_string(),
            },
        },
        Request::Players => Response::Players {
            players: players
                .lock()
                .expect("players lock poisoned")
                .iter()
                .map(|p| p.info.clone())
                .collect(),
        },
        Request::LastSaved => Response::LastSaved {
            at: *last_saved.lock().expect("last saved lock poisoned"),
        },
        Request::LastExit => Response::LastExit {
            info: last_exit.lock().expect("last exit lock poisoned").clone(),
        },
    };

    if let Err(e) = write_frame(&mut write_half, &response).await {
        tracing::warn!(error = %e, "failed to write control response");
    }
}

/// Refreshes `stats` on `STATS_REFRESH_INTERVAL`: sums `memory()`/
/// `cpu_usage()` over the Valheim child and its true (non-thread)
/// descendants, mirroring what `web::routes::resources` used to compute
/// centrally for every instance from `odin serve`'s own host-wide process
/// table — done here instead, scoped to just this one instance's process
/// tree, since `odin run` already knows its exact child pid. Runs forever;
/// aborted by `run_instance` alongside `log_poller` once the child exits.
async fn refresh_stats_periodically(pid: u32, stats: StatsHandle) {
    let mut system = System::new_all();
    loop {
        system = tokio::task::spawn_blocking(move || {
            system.refresh_all();
            system
        })
        .await
        .unwrap_or_else(|_| System::new_all());

        let mut cpu_percent = 0.0;
        let mut memory_bytes = 0;
        for descendant in process::descendant_pids(&system, &[pid]) {
            if let Some(p) = system.process(Pid::from_u32(descendant)) {
                cpu_percent += p.cpu_usage();
                memory_bytes += p.memory();
            }
        }
        *stats.lock().expect("stats lock poisoned") = Some((cpu_percent, memory_bytes));

        tokio::time::sleep(STATS_REFRESH_INTERVAL).await;
    }
}

async fn handle_events_connection(mut stream: UnixStream, mut rx: broadcast::Receiver<Event>) {
    loop {
        let event = match rx.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "closing lagged supervisor event stream");
                return;
            }
            Err(broadcast::error::RecvError::Closed) => return,
        };
        let is_exit = matches!(event, Event::Exited { .. });
        if write_frame(&mut stream, &event).await.is_err() || is_exit {
            return;
        }
    }
}

/// Polls `console.log` for newly-appended lines and broadcasts each one as
/// an `Event::LogLine`, exactly like `web::log_tail::tail_and_broadcast` did
/// when `odin serve` itself polled the file — that logic now lives here,
/// closer to the process actually producing the file, and `odin serve`
/// receives lines pushed over `events.sock` instead of polling on its own.
///
/// Also recognizes player joins/leaves (see `crate::player_events::
/// parse_line`), world saves (see `crate::save_events::
/// is_world_saved_line`), and readiness (see `crate::readiness_events::
/// is_ready_line`) in each line, maintaining `players`/`last_saved`/`ready`
/// and pushing the corresponding
/// `Event::PlayerJoined`/`PlayerLeft`/`WorldSaved` alongside the
/// unconditional `LogLine` for the same line — self-monitoring the same way
/// `refresh_stats_periodically` self-monitors CPU/memory, instead of `odin
/// serve` re-parsing raw lines it receives secondhand. Readiness has no
/// dedicated push event: it rides on the next `Ping`/`Pong` instead, which
/// `odin serve` already polls regularly.
///
/// Also keeps `recent_lines` (a bounded tail, capped at
/// `RECENT_LINES_CAPACITY`) so `run_instance` has something to snapshot
/// into `Request::LastExit`'s diagnostics whenever the child exits.
async fn poll_console_log(
    log_file: PathBuf,
    events_tx: broadcast::Sender<Event>,
    players: PlayersHandle,
    last_saved: SavedHandle,
    ready: ReadyHandle,
    recent_lines: RecentLinesHandle,
) {
    let mut pos = tokio::task::spawn_blocking({
        let log_file = log_file.clone();
        move || std::fs::metadata(&log_file).map(|m| m.len()).unwrap_or(0)
    })
    .await
    .unwrap_or(0);

    loop {
        tokio::time::sleep(LOG_POLL_INTERVAL).await;

        let file = log_file.clone();
        let (new_pos, chunk) =
            tokio::task::spawn_blocking(move || crate::log_poll::read_new_bytes(&file, pos))
                .await
                .unwrap_or((pos, String::new()));
        pos = new_pos;
        if chunk.is_empty() {
            continue;
        }

        for line in chunk.lines() {
            if let Some(event) = crate::player_events::parse_line(line)
                && let Some(pushed) = apply_player_event(&players, event)
            {
                let _ = events_tx.send(pushed);
            }
            if crate::save_events::is_world_saved_line(line) {
                let at = Utc::now();
                *last_saved.lock().expect("last saved lock poisoned") = Some(at);
                let _ = events_tx.send(Event::WorldSaved { at });
            }
            if crate::readiness_events::is_ready_line(line) {
                *ready.lock().expect("ready lock poisoned") = true;
            }
            {
                let mut recent = recent_lines.lock().expect("recent lines lock poisoned");
                if recent.len() >= RECENT_LINES_CAPACITY {
                    recent.pop_front();
                }
                recent.push_back(line.to_string());
            }
            let _ = events_tx.send(Event::LogLine {
                line: line.to_string(),
            });
        }
    }
}

/// Applies a recognized join/leave to `players`, returning the structured
/// `Event` to push if it actually changed the tracked list (a duplicate
/// join, or a leave for an untracked peer, is silently ignored — same
/// semantics as `web::players::PlayerRegistry::apply`).
fn apply_player_event(
    players: &PlayersHandle,
    event: crate::player_events::PlayerEvent,
) -> Option<Event> {
    use crate::player_events::{PlayerEvent, PlayerInfo};

    let mut players = players.lock().expect("players lock poisoned");
    match event {
        PlayerEvent::Joined { peer, name } => {
            if players.iter().any(|p| p.peer == peer) {
                return None;
            }
            players.push(TrackedPlayer {
                peer,
                info: PlayerInfo {
                    name: name.clone(),
                    connected_at: Utc::now(),
                },
            });
            Some(Event::PlayerJoined { name })
        }
        PlayerEvent::Left { peer } => {
            let index = players.iter().position(|p| p.peer == peer)?;
            let removed = players.remove(index);
            Some(Event::PlayerLeft {
                name: removed.info.name,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::client;

    fn temp_paths(label: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!(
            "odin-supervisor-server-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        }
    }

    fn append_line(path: &std::path::Path, line: &str) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{line}").unwrap();
    }

    #[tokio::test]
    async fn bind_private_refuses_a_live_socket() {
        let paths = temp_paths("bind-live");
        let socket_path = paths.runtime_dir().join("live.sock");
        std::fs::create_dir_all(socket_path.parent().unwrap()).unwrap();
        let listener = bind_private(&socket_path).unwrap();

        let result = bind_private(&socket_path);
        assert!(result.unwrap_err().to_string().contains("already in use"));

        drop(listener);
        std::fs::remove_file(socket_path).ok();
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    /// Unlike the resources refresher (which needs a live OS process
    /// tree), `poll_console_log`'s player/save detection only needs a real
    /// file and a channel — no Valheim binary required, so this runs by
    /// default rather than being gated behind `--ignored`.
    #[tokio::test]
    async fn poll_console_log_pushes_structured_player_events() {
        let dir = std::env::temp_dir().join(format!(
            "odin-poll-console-log-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let log_file = dir.join("console.log");
        std::fs::write(&log_file, "").unwrap();

        let (events_tx, mut events_rx) = broadcast::channel::<Event>(EVENT_BROADCAST_CAPACITY);
        let players: PlayersHandle = Arc::new(Mutex::new(Vec::new()));
        let last_saved: SavedHandle = Arc::new(Mutex::new(None));
        let ready: ReadyHandle = Arc::new(Mutex::new(false));
        let recent_lines: RecentLinesHandle =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let poller = tokio::spawn(poll_console_log(
            log_file.clone(),
            events_tx.clone(),
            players.clone(),
            last_saved.clone(),
            ready.clone(),
            recent_lines.clone(),
        ));

        // Give the poller a moment to record the file's starting length
        // before it grows. Appended, like the real console.log (never
        // rotated or truncated — see `web::log_tail`'s module doc comment).
        tokio::time::sleep(LOG_POLL_INTERVAL / 2).await;
        append_line(
            &log_file,
            "14:32:10: Got character ZDOID from client 0 : Bjorn",
        );

        let mut saw_joined = false;
        let mut saw_log_line = false;
        for _ in 0..2 {
            match tokio::time::timeout(Duration::from_secs(2), events_rx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                Event::PlayerJoined { name } => {
                    assert_eq!(name, "Bjorn");
                    saw_joined = true;
                }
                Event::LogLine { line } => {
                    assert!(line.contains("Bjorn"));
                    saw_log_line = true;
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(saw_joined && saw_log_line);
        assert_eq!(players.lock().unwrap().len(), 1);

        append_line(&log_file, "14:40:02: Closing socket 0");
        let mut saw_left = false;
        for _ in 0..2 {
            match tokio::time::timeout(Duration::from_secs(2), events_rx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                Event::PlayerLeft { name } => {
                    assert_eq!(name, "Bjorn");
                    saw_left = true;
                }
                Event::LogLine { .. } => {}
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(saw_left);
        assert!(players.lock().unwrap().is_empty());

        poller.abort();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn poll_console_log_pushes_world_saved_events() {
        let dir = std::env::temp_dir().join(format!(
            "odin-poll-console-log-save-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let log_file = dir.join("console.log");
        std::fs::write(&log_file, "").unwrap();

        let (events_tx, mut events_rx) = broadcast::channel::<Event>(EVENT_BROADCAST_CAPACITY);
        let players: PlayersHandle = Arc::new(Mutex::new(Vec::new()));
        let last_saved: SavedHandle = Arc::new(Mutex::new(None));
        let ready: ReadyHandle = Arc::new(Mutex::new(false));
        let recent_lines: RecentLinesHandle =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let poller = tokio::spawn(poll_console_log(
            log_file.clone(),
            events_tx.clone(),
            players.clone(),
            last_saved.clone(),
            ready.clone(),
            recent_lines.clone(),
        ));

        assert!(last_saved.lock().unwrap().is_none());

        tokio::time::sleep(LOG_POLL_INTERVAL / 2).await;
        append_line(&log_file, "14:32:00: World saved");

        let mut saw_saved = false;
        let mut saw_log_line = false;
        for _ in 0..2 {
            match tokio::time::timeout(Duration::from_secs(2), events_rx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                Event::WorldSaved { .. } => saw_saved = true,
                Event::LogLine { line } => {
                    assert!(line.contains("World saved"));
                    saw_log_line = true;
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert!(saw_saved && saw_log_line);
        assert!(last_saved.lock().unwrap().is_some());

        poller.abort();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Readiness has no dedicated push event (see `poll_console_log`'s doc
    /// comment) — it rides on the next `Ping`, so this checks the shared
    /// `ReadyHandle` directly instead of the events channel.
    #[tokio::test]
    async fn poll_console_log_flips_ready_on_the_readiness_line() {
        let dir = std::env::temp_dir().join(format!(
            "odin-poll-console-log-ready-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let log_file = dir.join("console.log");
        std::fs::write(&log_file, "").unwrap();

        let (events_tx, _events_rx) = broadcast::channel::<Event>(EVENT_BROADCAST_CAPACITY);
        let players: PlayersHandle = Arc::new(Mutex::new(Vec::new()));
        let last_saved: SavedHandle = Arc::new(Mutex::new(None));
        let ready: ReadyHandle = Arc::new(Mutex::new(false));
        let recent_lines: RecentLinesHandle =
            Arc::new(Mutex::new(std::collections::VecDeque::new()));
        let poller = tokio::spawn(poll_console_log(
            log_file.clone(),
            events_tx.clone(),
            players.clone(),
            last_saved.clone(),
            ready.clone(),
            recent_lines.clone(),
        ));

        assert!(!*ready.lock().unwrap());

        tokio::time::sleep(LOG_POLL_INTERVAL / 2).await;
        append_line(&log_file, "08/29 20:18:16: Game server connected");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while !*ready.lock().unwrap() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "ready was never set after the readiness line"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        poller.abort();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// End-to-end against the *real* `valheim_server.x86_64` on this host
    /// (symlinked in, not copied): runs the supervisor loop directly (as
    /// `odin run` itself would, minus the process re-exec — a `cargo test`
    /// binary can't usefully exec itself as `odin`; that leg is covered by
    /// manual verification instead), pings it for the real pid, waits for a
    /// real console log line pushed over the events socket, then stops it
    /// and confirms both processes exit and every socket/pidfile is
    /// cleaned up. Skipped by default (needs a real Valheim dedicated
    /// server install, like the steamcmd-dependent tests elsewhere in the
    /// crate): `cargo test -- --ignored run_instance_e2e`.
    #[tokio::test]
    #[ignore]
    async fn run_instance_e2e_against_a_real_server_binary() {
        // Deliberately bypasses `Paths::resolve`'s system-mode detection
        // (this dev box may also have a package-installed
        // `/etc/odin/config.toml` pointing at `/var/lib/odin`, unreadable by
        // this user) — this test only cares about finding *a* real install
        // to symlink in, always under the per-user XDG data dir regardless
        // of system-mode.
        let real_install = directories::ProjectDirs::from("", "", "odin")
            .unwrap()
            .data_dir()
            .join("install")
            .join("valheim");
        assert!(
            real_install.join("valheim_server.x86_64").is_file(),
            "expected a real Valheim install at {}; run `odin install` first",
            real_install.display()
        );

        let paths = temp_paths("run-e2e");
        std::fs::create_dir_all(paths.shared_install_dir().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real_install, paths.shared_install_dir()).unwrap();

        let supervisor_task = tokio::spawn(run_instance(paths.clone(), "e2e-run"));

        let response = client::ping_with_retry(&paths, "e2e-run", Duration::from_secs(10))
            .await
            .unwrap();
        let (pid, pid_started_at) = match response {
            Response::Pong {
                pid,
                pid_started_at,
                ..
            } => (pid, pid_started_at),
            other => panic!("expected Pong, got {other:?}"),
        };
        assert!(process::is_alive(pid, pid_started_at));

        // `ready` starts false and stays that way until the supervisor's
        // own parsing sees "Game server connected" — deterministic and
        // provable here without depending on a real Steam GameServer login
        // actually succeeding (this dev sandbox's network doesn't let one
        // through, confirmed via `[Steamworks.NET] GameServer.Init()
        // failed.` in console.log — a UDP restriction, not a bug: the same
        // parsing logic against a synthetic "Game server connected" line is
        // covered deterministically by
        // `poll_console_log_pushes_ready_events` below, and the real
        // end-to-end transition is left to manual verification against an
        // install with working Steam connectivity).
        assert!(
            matches!(
                client::ping(&paths, "e2e-run").await,
                Ok(Response::Pong { ready: false, .. })
            ),
            "expected a freshly spawned instance to report ready: false"
        );

        // Past one STATS_REFRESH_INTERVAL, so the background refresher has
        // had time to complete its first pass.
        tokio::time::sleep(STATS_REFRESH_INTERVAL + Duration::from_secs(1)).await;
        let stats_paths = paths.clone();
        let stats_response = tokio::task::spawn_blocking(move || {
            client::stats_blocking(&stats_paths, "e2e-run", Duration::from_secs(2))
        })
        .await
        .unwrap()
        .unwrap();
        assert!(
            matches!(stats_response, Response::Stats { memory_bytes, .. } if memory_bytes > 0),
            "expected Stats with nonzero memory_bytes, got {stats_response:?}"
        );

        client::stop(&paths, "e2e-run", 30).await.unwrap();
        supervisor_task.await.unwrap().unwrap();

        assert!(!process::is_alive(pid, pid_started_at));
        assert!(!super::super::control_sock_path(&paths, "e2e-run").exists());
        assert!(!super::super::events_sock_path(&paths, "e2e-run").exists());
        assert!(!super::super::pidfile_path(&paths, "e2e-run").exists());

        let reloaded =
            crate::instance::Instance::load_existing(&paths, &Db::open(&paths).unwrap(), "e2e-run")
                .unwrap();
        assert!(!lifecycle::is_running(&reloaded).unwrap());
        assert_eq!(reloaded.state.pid, None);

        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    /// End-to-end against the real Valheim binary, same setup as
    /// `run_instance_e2e_against_a_real_server_binary`: kills the running
    /// process directly (simulating an unwitnessed crash — an OOM kill or
    /// an external `kill -9`, not the supervisor's own `Stop` path) for an
    /// instance with `auto_restart` enabled, then confirms the *same*
    /// supervisor process respawns it in place — a `Pong` with a different
    /// pid than before, without ever going through
    /// `instance::lifecycle::start` (which is what `odin serve`'s slower,
    /// polling-based fallback would use instead, if this path didn't
    /// exist). Skipped by default, same as the other e2e test: `cargo test
    /// -- --ignored run_instance_e2e`.
    #[tokio::test]
    #[ignore]
    async fn run_instance_e2e_auto_restarts_after_an_unexpected_exit() {
        let real_install = directories::ProjectDirs::from("", "", "odin")
            .unwrap()
            .data_dir()
            .join("install")
            .join("valheim");
        assert!(
            real_install.join("valheim_server.x86_64").is_file(),
            "expected a real Valheim install at {}; run `odin install` first",
            real_install.display()
        );

        let paths = temp_paths("run-e2e-auto-restart");
        std::fs::create_dir_all(paths.shared_install_dir().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real_install, paths.shared_install_dir()).unwrap();

        let db = Db::open(&paths).unwrap();
        let mut instance = Instance::create(&paths, &db, "e2e-auto-restart").unwrap();
        instance.state.auto_restart = true;
        instance.save(&db).unwrap();
        drop(db);

        let supervisor_task = tokio::spawn(run_instance(paths.clone(), "e2e-auto-restart"));

        let response = client::ping_with_retry(&paths, "e2e-auto-restart", Duration::from_secs(10))
            .await
            .unwrap();
        let (first_pid, first_pid_started_at) = match response {
            Response::Pong {
                pid,
                pid_started_at,
                ..
            } => (pid, pid_started_at),
            other => panic!("expected Pong, got {other:?}"),
        };
        assert!(process::is_alive(first_pid, first_pid_started_at));

        process::send_signal(first_pid, first_pid_started_at, Signal::Kill).unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let (second_pid, second_pid_started_at) = loop {
            assert!(
                tokio::time::Instant::now() < deadline,
                "supervisor did not respawn the instance within the deadline"
            );
            if let Ok(Response::Pong {
                pid,
                pid_started_at,
                ..
            }) = client::ping(&paths, "e2e-auto-restart").await
                && pid != first_pid
            {
                break (pid, pid_started_at);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        assert!(process::is_alive(second_pid, second_pid_started_at));
        assert!(!process::is_alive(first_pid, first_pid_started_at));

        // The control socket stays alive across an in-place restart
        // (unlike after a real `Stop`), so this is the one real-process
        // window where `LastExit` is both populated and still reachable —
        // proving the unconditional exit-diagnostics snapshot in the
        // `child.wait()` branch actually ran for a real SIGKILL, not just
        // the synthetic line fed to it in `poll_console_log`'s own test.
        match client::last_exit(&paths, "e2e-auto-restart").await {
            Ok(Response::LastExit { info: Some(info) }) => {
                assert!(
                    info.code.is_none(),
                    "a SIGKILL'd process has no exit code, got {:?}",
                    info.code
                );
            }
            other => panic!("expected LastExit with recorded info, got {other:?}"),
        }

        client::stop(&paths, "e2e-auto-restart", 30).await.unwrap();
        supervisor_task.await.unwrap().unwrap();

        assert!(!process::is_alive(second_pid, second_pid_started_at));
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }
}
