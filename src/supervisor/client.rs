//! The `odin serve` side of the supervisor RPC: the podman-equivalent role.
//! Spawns `odin run --instance <name>` detached and talks to its
//! control/events sockets. Not yet called by `instance::lifecycle` or
//! `odin serve` — see the phased rollout in the supervisor design plan.
//!
//! Fully exercised by this module's own tests in the meantime; the
//! `expect(dead_code)` below is expected to start failing (a good thing —
//! it's a forcing function) the moment `instance::lifecycle` is wired to
//! call into this module in a follow-up phase, at which point it should be
//! removed.
#![expect(
    dead_code,
    reason = "not yet called from instance::lifecycle/odin serve; see module doc"
)]

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

/// Connects to `instance_name`'s control socket, retrying at a fixed
/// interval until it succeeds or `timeout` elapses. Meant to be called
/// right after `spawn_detached`: bounded by `odin run`'s own startup time
/// (binding its sockets happens before it execs Valheim), not by how long
/// the game server itself takes to come up.
pub async fn connect_control_with_retry(
    paths: &Paths,
    instance_name: &str,
    timeout: Duration,
) -> Result<UnixStream> {
    let path = super::control_sock_path(paths, instance_name);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match UnixStream::connect(&path).await {
            Ok(stream) => return Ok(stream),
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(CONNECT_RETRY_INTERVAL).await;
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "timed out connecting to {} for instance '{instance_name}'",
                        path.display()
                    )
                });
            }
        }
    }
}

/// Sends `Ping` over a fresh connection to `instance_name`'s control
/// socket and returns the response. Fails fast (no retry) — callers that
/// just spawned the supervisor should use `connect_control_with_retry`
/// first; this is for an already-presumed-live supervisor.
pub async fn ping(paths: &Paths, instance_name: &str) -> Result<Response> {
    let mut stream = UnixStream::connect(super::control_sock_path(paths, instance_name))
        .await
        .context("failed to connect to control socket")?;
    request(&mut stream, &Request::Ping).await
}

/// Asks the supervisor to stop the instance (SIGINT, then SIGKILL after
/// `timeout_secs`) and exit. Returns once the supervisor has acknowledged
/// the request — not once the process has actually exited (the supervisor
/// itself removes the DB pid and its socket/pidfiles once it does).
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

    #[tokio::test]
    async fn connect_control_with_retry_succeeds_once_the_socket_appears() {
        let paths = temp_paths("retry");
        let sock_path = super::super::control_sock_path(&paths, "client-test-retry");
        std::fs::create_dir_all(sock_path.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&sock_path);

        let bind_task = tokio::spawn({
            let sock_path = sock_path.clone();
            async move {
                // Simulate `odin run` taking a moment to start up before
                // binding its socket.
                tokio::time::sleep(Duration::from_millis(100)).await;
                let listener = UnixListener::bind(&sock_path).unwrap();
                let _ = listener.accept().await;
            }
        });

        let stream =
            connect_control_with_retry(&paths, "client-test-retry", Duration::from_secs(2)).await;
        assert!(stream.is_ok());

        bind_task.await.unwrap();
        let _ = std::fs::remove_file(&sock_path);
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[tokio::test]
    async fn connect_control_with_retry_times_out_if_nothing_ever_binds() {
        let paths = temp_paths("timeout");
        let result =
            connect_control_with_retry(&paths, "client-test-timeout", Duration::from_millis(150))
                .await;
        assert!(result.is_err());
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[tokio::test]
    async fn subscribe_events_yields_pushed_events_until_disconnect() {
        let paths = temp_paths("events");
        let sock_path = super::super::events_sock_path(&paths, "client-test-events");
        let listener = bind_fresh(&sock_path);

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            write_frame(
                &mut stream,
                &Event::LogLine {
                    line: "hello".to_string(),
                },
            )
            .await
            .unwrap();
            write_frame(&mut stream, &Event::Exited { code: Some(0) })
                .await
                .unwrap();
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        let stream = subscribe_events(&paths, "client-test-events")
            .await
            .unwrap();
        futures_util::pin_mut!(stream);
        use futures_util::StreamExt as _;

        let first = stream.next().await.unwrap();
        assert!(matches!(first, Event::LogLine { line } if line == "hello"));
        let second = stream.next().await.unwrap();
        assert!(matches!(second, Event::Exited { code: Some(0) }));
        assert!(stream.next().await.is_none());

        server.await.unwrap();
        let _ = std::fs::remove_file(&sock_path);
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }
}
