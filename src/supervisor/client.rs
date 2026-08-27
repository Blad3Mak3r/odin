//! The `odin serve` side of the supervisor RPC: the podman-equivalent role.
//! Spawns `odin run --instance <name>` detached and talks to its
//! control/events sockets. `spawn_detached`/`ping`/`ping_with_retry`/`stop`
//! are wired into `instance::lifecycle`; `subscribe_events` (the
//! `LogTailRegistry` event bridge) lands in a follow-up phase — see that
//! function's doc comment.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::Stream;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::process::Command;

use super::protocol::{Event, Request, Response, read_frame, write_frame};
use crate::paths::Paths;

const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// Spawns `odin run --instance <name>` detached: its own process group
/// (same mechanism `instance::process::build_command` already uses for the
/// Valheim child itself) and stdio discarded — the supervisor logs via
/// `tracing` to its own file, not this process's stdout/stderr. The `Child`
/// handle is dropped immediately; like the Valheim child today, `kill_on_drop`
/// defaults to `false`, so this does not kill the supervisor.
pub async fn spawn_detached(instance_name: &str) -> Result<()> {
    let exe = std::env::current_exe().context("failed to resolve odin's own executable path")?;
    let mut cmd = Command::new(exe);
    cmd.arg("run")
        .arg("--instance")
        .arg(instance_name)
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn 'odin run --instance {instance_name}'"))?;
    drop(child);
    Ok(())
}

/// Sends `Ping` over a fresh connection to `instance_name`'s control
/// socket and returns the response. Fails immediately (no retry) if the
/// socket doesn't exist or nothing answers — see `ping_with_retry` for
/// waiting out a just-spawned supervisor's startup time.
pub async fn ping(paths: &Paths, instance_name: &str) -> Result<Response> {
    let mut stream = UnixStream::connect(super::control_sock_path(paths, instance_name))
        .await
        .context("failed to connect to control socket")?;
    request(&mut stream, &Request::Ping).await
}

/// Pings `instance_name`'s control socket, retrying at a fixed interval
/// until it responds or `timeout` elapses. Meant to be called right after
/// `spawn_detached`: bounded by `odin run`'s own startup time, not by how
/// long the game server itself takes to come up. A successful response (not
/// just a successful `connect`) is the signal to wait for — `odin run`
/// binds its sockets before it execs Valheim, so an early `connect` can
/// succeed (the kernel queues it) well before `set_pid` has actually run;
/// only a real reply proves the supervisor has reached its serving loop.
pub async fn ping_with_retry(
    paths: &Paths,
    instance_name: &str,
    timeout: Duration,
) -> Result<Response> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match ping(paths, instance_name).await {
            Ok(response) => return Ok(response),
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(CONNECT_RETRY_INTERVAL).await;
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("timed out waiting for supervisor to become ready for instance '{instance_name}'")
                });
            }
        }
    }
}

/// Synchronous ping, for the telemetry tick's `spawn_blocking` context
/// (`web::routes::resources::compute_instance_snapshot`), where spinning up
/// a `tokio::net::UnixStream` would be pointless — plain blocking I/O with a
/// short timeout is simpler and just as correct there. A slow/wedged
/// supervisor reads as "unreachable" rather than stalling the tick.
pub fn ping_blocking(paths: &Paths, instance_name: &str, timeout: Duration) -> Result<Response> {
    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixStream as StdUnixStream;

    let mut stream = StdUnixStream::connect(super::control_sock_path(paths, instance_name))
        .context("failed to connect to control socket")?;
    stream
        .set_read_timeout(Some(timeout))
        .context("failed to set read timeout")?;
    stream
        .set_write_timeout(Some(timeout))
        .context("failed to set write timeout")?;

    let mut line = serde_json::to_string(&Request::Ping).context("failed to encode Ping")?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .context("failed to write Ping")?;

    let mut response_line = String::new();
    std::io::BufReader::new(stream)
        .read_line(&mut response_line)
        .context("failed to read ping response")?;
    serde_json::from_str(response_line.trim_end()).context("failed to decode ping response")
}

/// Asks the supervisor to stop the instance (SIGINT, then SIGKILL after
/// `timeout_secs`) and exit. Returns once the supervisor has acknowledged
/// the request — not once the process has actually exited (the supervisor
/// itself removes the DB pid and its socket/pidfiles once it does);
/// `instance::lifecycle::stop` waits for the actual exit separately.
pub async fn stop(paths: &Paths, instance_name: &str, timeout_secs: u64) -> Result<()> {
    let mut stream = UnixStream::connect(super::control_sock_path(paths, instance_name))
        .await
        .context("failed to connect to control socket")?;
    match request(&mut stream, &Request::Stop { timeout_secs }).await? {
        Response::Stopped => Ok(()),
        Response::Error { message } => bail!(message),
        other => bail!("unexpected response to Stop: {other:?}"),
    }
}

async fn request(stream: &mut UnixStream, req: &Request) -> Result<Response> {
    write_frame(stream, req).await?;
    let mut reader = BufReader::new(stream);
    read_frame::<Response, _>(&mut reader)
        .await?
        .context("supervisor closed the connection without responding")
}

/// Subscribes to `instance_name`'s events socket, yielding pushed
/// `LogLine`/`Exited` events as they arrive. The stream ends when the
/// supervisor closes the connection (normally right after `Exited`).
///
/// Not yet called from anywhere — becomes the `LogTailRegistry` event
/// bridge in a follow-up phase (replacing `web::log_tail`'s file-polling
/// for instances with a live control socket). The `expect` below is a
/// forcing function: it starts failing the moment something calls this,
/// which is the cue to remove it.
#[expect(
    dead_code,
    reason = "wired into the LogTailRegistry event bridge in a follow-up phase"
)]
pub async fn subscribe_events(
    paths: &Paths,
    instance_name: &str,
) -> Result<impl Stream<Item = Event> + use<>> {
    let stream = UnixStream::connect(super::events_sock_path(paths, instance_name))
        .await
        .context("failed to connect to events socket")?;
    let mut reader = BufReader::new(stream);
    Ok(async_stream::stream! {
        while let Ok(Some(event)) = read_frame::<Event, _>(&mut reader).await {
            yield event;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    fn temp_paths(label: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!(
            "odin-supervisor-client-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        }
    }

    /// Binds a Unix socket at `path`, removing any file a previous, crashed
    /// test run left behind first — the real `bind_private` (`server.rs`)
    /// does the same for the same reason.
    fn bind_fresh(path: &std::path::Path) -> UnixListener {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(path);
        UnixListener::bind(path).unwrap()
    }

    /// A minimal fake supervisor: accepts one control connection, replies
    /// to whatever request it gets with a canned `Pong`, then stops
    /// serving. Exercises `client::ping`/`client::request` end to end
    /// against a real Unix socket without needing `instance::process` or a
    /// real Valheim binary at all.
    async fn fake_supervisor_once(paths: &Paths, instance_name: &str, response: Response) {
        let sock_path = super::super::control_sock_path(paths, instance_name);
        let listener = bind_fresh(&sock_path);
        let (stream, _) = listener.accept().await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let _req = read_frame::<Request, _>(&mut reader).await.unwrap();
        write_frame(&mut write_half, &response).await.unwrap();
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn ping_returns_the_supervisors_response() {
        let paths = temp_paths("ping");
        let server = tokio::spawn({
            let paths = paths.clone();
            async move {
                fake_supervisor_once(
                    &paths,
                    "client-test-ping",
                    Response::Pong {
                        pid: 1234,
                        pid_started_at: 999,
                        started_at: chrono::Utc::now(),
                    },
                )
                .await;
            }
        });

        // Give the fake supervisor a moment to bind before connecting.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let response = ping(&paths, "client-test-ping").await.unwrap();
        assert!(matches!(response, Response::Pong { pid: 1234, .. }));

        server.await.unwrap();
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[tokio::test]
    async fn ping_with_retry_succeeds_once_the_supervisor_starts_answering() {
        let paths = temp_paths("ping-retry");
        let server = tokio::spawn({
            let paths = paths.clone();
            async move {
                // Simulate `odin run` taking a moment to reach its serving
                // loop after its socket already exists.
                tokio::time::sleep(Duration::from_millis(150)).await;
                fake_supervisor_once(
                    &paths,
                    "client-test-ping-retry",
                    Response::Pong {
                        pid: 4321,
                        pid_started_at: 111,
                        started_at: chrono::Utc::now(),
                    },
                )
                .await;
            }
        });

        let response =
            ping_with_retry(&paths, "client-test-ping-retry", Duration::from_secs(2)).await;
        assert!(matches!(response, Ok(Response::Pong { pid: 4321, .. })));

        server.await.unwrap();
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[tokio::test]
    async fn ping_with_retry_times_out_if_nothing_ever_answers() {
        let paths = temp_paths("ping-timeout");
        let result = ping_with_retry(
            &paths,
            "client-test-ping-timeout",
            Duration::from_millis(150),
        )
        .await;
        assert!(result.is_err());
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn ping_blocking_returns_the_supervisors_response() {
        let paths = temp_paths("ping-blocking");
        let sock_path = super::super::control_sock_path(&paths, "client-test-ping-blocking");
        std::fs::create_dir_all(sock_path.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&sock_path);
        let listener = std::os::unix::net::UnixListener::bind(&sock_path).unwrap();

        let server = std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut writer = stream;
            let mut response = serde_json::to_string(&Response::Pong {
                pid: 555,
                pid_started_at: 42,
                started_at: chrono::Utc::now(),
            })
            .unwrap();
            response.push('\n');
            writer.write_all(response.as_bytes()).unwrap();
        });

        let response =
            ping_blocking(&paths, "client-test-ping-blocking", Duration::from_secs(2)).unwrap();
        assert!(matches!(response, Response::Pong { pid: 555, .. }));

        server.join().unwrap();
        let _ = std::fs::remove_file(&sock_path);
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn ping_blocking_fails_fast_when_nothing_is_listening() {
        let paths = temp_paths("ping-blocking-fail");
        let result = ping_blocking(
            &paths,
            "client-test-ping-blocking-fail",
            Duration::from_millis(200),
        );
        assert!(result.is_err());
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[tokio::test]
    async fn stop_returns_ok_on_stopped_response() {
        let paths = temp_paths("stop-ok");
        let server = tokio::spawn({
            let paths = paths.clone();
            async move {
                fake_supervisor_once(&paths, "client-test-stop-ok", Response::Stopped).await;
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        stop(&paths, "client-test-stop-ok", 30).await.unwrap();

        server.await.unwrap();
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[tokio::test]
    async fn stop_returns_err_on_error_response() {
        let paths = temp_paths("stop-err");
        let server = tokio::spawn({
            let paths = paths.clone();
            async move {
                fake_supervisor_once(
                    &paths,
                    "client-test-stop-err",
                    Response::Error {
                        message: "boom".to_string(),
                    },
                )
                .await;
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        let err = stop(&paths, "client-test-stop-err", 30).await.unwrap_err();
        assert!(err.to_string().contains("boom"));

        server.await.unwrap();
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }
}
