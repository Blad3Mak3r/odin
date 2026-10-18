//! The `odin serve` side of the runtime RPC: talks to the control socket of
//! the `odin run` runtime inside a game container.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::BufReader;
use tokio::net::UnixStream;

use super::protocol::{MAX_FRAME_BYTES, Request, Response, read_frame, write_frame};

const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(50);
const CONTROL_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Synchronous ping, for the telemetry tick's `spawn_blocking` context, where
/// spinning up a `tokio::net::UnixStream` would be pointless — plain blocking
/// I/O with a short timeout is simpler and just as correct there. A slow or
/// wedged runtime reads as "unreachable" rather than stalling the tick.
pub fn ping_socket_blocking(
    control_socket: &std::path::Path,
    timeout: Duration,
) -> Result<Response> {
    request_socket_blocking(control_socket, timeout, &Request::Ping)
}

pub fn stats_socket_blocking(
    control_socket: &std::path::Path,
    timeout: Duration,
) -> Result<Response> {
    request_socket_blocking(control_socket, timeout, &Request::Stats)
}

fn request_socket_blocking(
    control_socket: &std::path::Path,
    timeout: Duration,
    req: &Request,
) -> Result<Response> {
    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixStream as StdUnixStream;

    let mut stream =
        StdUnixStream::connect(control_socket).context("failed to connect to control socket")?;
    stream
        .set_read_timeout(Some(timeout))
        .context("failed to set read timeout")?;
    stream
        .set_write_timeout(Some(timeout))
        .context("failed to set write timeout")?;

    let mut line = serde_json::to_string(req).context("failed to encode request")?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .context("failed to write request")?;

    let mut response_line = String::new();
    let bytes_read = std::io::BufReader::new(stream)
        .read_line(&mut response_line)
        .context("failed to read response")?;
    if bytes_read > MAX_FRAME_BYTES {
        bail!("runtime frame exceeds {MAX_FRAME_BYTES} bytes");
    }
    serde_json::from_str(response_line.trim_end()).context("failed to decode response")
}

pub async fn stop_socket(control_socket: &std::path::Path, timeout_secs: u64) -> Result<()> {
    match request_socket(
        control_socket,
        &Request::Stop { timeout_secs },
        CONTROL_REQUEST_TIMEOUT,
    )
    .await?
    {
        Response::Stopped => Ok(()),
        Response::Error { message, .. } => bail!(message),
        other => bail!("unexpected response to Stop: {other:?}"),
    }
}

pub async fn update_socket(control_socket: &std::path::Path) -> Result<()> {
    match request_socket(
        control_socket,
        &Request::Update,
        Duration::from_secs(15 * 60),
    )
    .await?
    {
        Response::Updated => Ok(()),
        Response::Error { message, .. } => bail!(message),
        other => bail!("unexpected response to Update: {other:?}"),
    }
}

/// Pings a runtime control socket once without waiting for startup retries.
pub async fn ping_socket(control_socket: &std::path::Path) -> Result<Response> {
    request_socket(control_socket, &Request::Ping, CONTROL_REQUEST_TIMEOUT).await
}

/// Pings until the runtime answers. After every failed attempt `still_wanted`
/// runs, so the caller can give up early with its own error (for example when
/// the container has already exited) instead of waiting out the full timeout.
pub async fn ping_socket_with_retry<F, Fut>(
    control_socket: &std::path::Path,
    timeout: Duration,
    mut still_wanted: F,
) -> Result<Response>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let attempt = request_socket(
            control_socket,
            &Request::Ping,
            remaining.min(CONTROL_REQUEST_TIMEOUT),
        )
        .await;
        match attempt {
            Ok(response) => return Ok(response),
            Err(_) if tokio::time::Instant::now() < deadline => {
                still_wanted().await?;
                tokio::time::sleep(CONNECT_RETRY_INTERVAL).await;
            }
            Err(error) => {
                return Err(error).context(format!(
                    "timed out after {}s waiting for the container runtime socket",
                    timeout.as_secs()
                ));
            }
        }
    }
}

async fn request_socket(
    control_socket: &std::path::Path,
    req: &Request,
    timeout: Duration,
) -> Result<Response> {
    tokio::time::timeout(timeout, async {
        let mut stream = UnixStream::connect(control_socket)
            .await
            .context("failed to connect to control socket")?;
        write_frame(&mut stream, req).await?;
        let mut reader = BufReader::new(&mut stream);
        read_frame::<Response, _>(&mut reader)
            .await?
            .context("runtime closed the connection without responding")
    })
    .await
    .context("timed out waiting for runtime control response")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, Write};
    use std::os::unix::net::UnixListener;

    fn socket_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("odin-client-{label}-{}.sock", uuid::Uuid::new_v4()))
    }

    #[test]
    fn ping_socket_blocking_returns_the_runtimes_response() {
        let socket = socket_path("ping");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake runtime socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            std::io::BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let pong = serde_json::to_string(&Response::Pong {
                pid: 7,
                pid_started_at: 1,
                started_at: chrono::Utc::now(),
                protocol_version: super::super::protocol::PROTOCOL_VERSION,
                odin_version: Some("test".into()),
                ready: true,
            })
            .unwrap();
            stream.write_all(format!("{pong}\n").as_bytes()).unwrap();
            request
        });

        let response = ping_socket_blocking(&socket, Duration::from_secs(2)).unwrap();

        assert!(matches!(response, Response::Pong { pid: 7, .. }));
        assert!(server.join().unwrap().contains("\"ping\""));
        std::fs::remove_file(socket).ok();
    }

    #[test]
    fn ping_socket_blocking_fails_fast_when_nothing_is_listening() {
        let socket = socket_path("absent");

        assert!(ping_socket_blocking(&socket, Duration::from_millis(100)).is_err());
    }
}
