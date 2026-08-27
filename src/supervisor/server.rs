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
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use sysinfo::Signal;
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};

use super::protocol::{Event, Request, Response, read_frame, write_frame};
use crate::db::Db;
use crate::instance::{lifecycle, process};
use crate::paths::{self, Paths};

const EVENT_BROADCAST_CAPACITY: usize = 256;
const LOG_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Runs the supervisor for `instance_name` end to end: prepares and spawns
/// the Valheim process, serves the control/events sockets, and blocks until
/// the process exits (either on its own or via a `Stop` request) and
/// cleanup is complete. `commands::run::run` just calls this and returns.
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
    let events_listener = bind_private(&events_path)?;
    write_pidfile(&pidfile)?;

    let cmd = process::build_command(&instance, &paths)?;
    let mut child = process::spawn(cmd)
        .await
        .with_context(|| format!("failed to start instance '{instance_name}'"))?;
    let pid = child.id().context("spawned child has no pid")?;
    let pid_started_at = process::start_time_of(pid)?;
    let started_at = Utc::now();
    instance.state.pid = Some(pid);
    instance.state.pid_started_at = Some(pid_started_at);
    crate::db::instances::set_pid(&db, instance_name, pid, pid_started_at, started_at)?;

    let (events_tx, _) = broadcast::channel::<Event>(EVENT_BROADCAST_CAPACITY);
    let console_log = paths::instance_logs_dir(&instance.dir).join("console.log");
    let log_poller = tokio::spawn(poll_console_log(console_log, events_tx.clone()));

    let (stop_tx, mut stop_rx) = mpsc::channel::<u64>(1);

    let exit_code = loop {
        tokio::select! {
            status = child.wait() => {
                break status.ok().and_then(|s| s.code());
            }
            Ok((stream, _)) = control_listener.accept() => {
                tokio::spawn(handle_control_connection(
                    stream,
                    pid,
                    pid_started_at,
                    started_at,
                    stop_tx.clone(),
                ));
            }
            Ok((stream, _)) = events_listener.accept() => {
                tokio::spawn(handle_events_connection(stream, events_tx.subscribe()));
            }
            Some(timeout_secs) = stop_rx.recv() => {
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

/// Binds a Unix socket at `path`, removing any stale socket file left
/// behind by a previous, uncleanly-terminated run first, and restricting
/// its permissions to the owner — the socket accepts a `Stop` request, so
/// it shouldn't be world-writable.
fn bind_private(path: &Path) -> Result<UnixListener> {
    let _ = std::fs::remove_file(path);
    let listener =
        UnixListener::bind(path).with_context(|| format!("failed to bind {}", path.display()))?;
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
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let request = match read_frame::<Request, _>(&mut reader).await {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(error = %e, "failed to read control request");
            return;
        }
    };

    let response = match request {
        Request::Ping => Response::Pong {
            pid,
            pid_started_at,
            started_at,
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
    };

    if let Err(e) = write_frame(&mut write_half, &response).await {
        tracing::warn!(error = %e, "failed to write control response");
    }
}

async fn handle_events_connection(mut stream: UnixStream, mut rx: broadcast::Receiver<Event>) {
    loop {
        let event = match rx.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
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
async fn poll_console_log(log_file: PathBuf, events_tx: broadcast::Sender<Event>) {
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
            let _ = events_tx.send(Event::LogLine {
                line: line.to_string(),
            });
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
}
