//! Wire protocol between `odin serve` (client, `super::client`) and `odin
//! run` (per-instance supervisor, server side, `super::server`): newline-
//! delimited JSON, one message per line. Deliberately no heavier framing
//! (length-prefixing, a binary codec) — messages are small, and NDJSON is
//! trivially debuggable with `socat`/`nc` against a running instance's
//! socket, which matters more at this project's scale than shaving bytes.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

/// A request sent over an instance's control socket. One request per
/// connection: the client connects, sends one `Request`, reads one
/// `Response`, and disconnects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Are you alive, and what's the Valheim process's identity?
    Ping,
    /// Stop the Valheim process (SIGINT, then SIGKILL after `timeout_secs`)
    /// and exit once it's gone.
    Stop { timeout_secs: u64 },
    /// Current resource usage of the Valheim child and its true
    /// descendants, as last computed by the supervisor's own background
    /// refresher — never computed synchronously on request, so this can't
    /// block the connection on a fresh `sysinfo` walk.
    Stats,
}

/// The reply to a `Request`, over the same control-socket connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong {
        pid: u32,
        pid_started_at: i64,
        started_at: DateTime<Utc>,
    },
    Stopped,
    /// Summed CPU/memory for the Valheim process plus any real child
    /// processes it has (never threads — see
    /// `instance::process::descendant_pids`). Replied with `Error` instead
    /// if the supervisor hasn't completed its first background refresh yet.
    Stats {
        cpu_percent: f32,
        memory_bytes: u64,
    },
    Error {
        message: String,
    },
}

/// A message pushed unprompted over an instance's events socket. One
/// long-lived connection per subscriber; the supervisor closes the
/// connection right after sending `Exited`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    LogLine { line: String },
    Exited { code: Option<i32> },
}

/// Reads one newline-delimited JSON message. `Ok(None)` means the peer
/// closed the connection cleanly (no partial trailing data) rather than
/// sending a final message — not an error.
pub async fn read_frame<T, R>(reader: &mut R) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
    R: AsyncBufReadExt + Unpin,
{
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .await
        .context("failed to read a frame from the supervisor socket")?;
    if n == 0 {
        return Ok(None);
    }
    let value = serde_json::from_str(line.trim_end())
        .with_context(|| format!("failed to decode frame: {line:?}"))?;
    Ok(Some(value))
}

pub async fn write_frame<T, W>(writer: &mut W, value: &T) -> Result<()>
where
    T: Serialize,
    W: AsyncWriteExt + Unpin,
{
    let mut line = serde_json::to_string(value).context("failed to encode frame as JSON")?;
    line.push('\n');
    writer
        .write_all(line.as_bytes())
        .await
        .context("failed to write a frame to the supervisor socket")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn request_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(&mut client, &Request::Stop { timeout_secs: 30 })
            .await
            .unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Request, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Request::Stop { timeout_secs: 30 })));
    }

    #[tokio::test]
    async fn response_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        let response = Response::Pong {
            pid: 4242,
            pid_started_at: 1_700_000_000,
            started_at: Utc::now(),
        };
        write_frame(&mut server, &response).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut client);
        let received = read_frame::<Response, _>(&mut reader).await.unwrap();
        match received {
            Some(Response::Pong {
                pid,
                pid_started_at,
                ..
            }) => {
                assert_eq!(pid, 4242);
                assert_eq!(pid_started_at, 1_700_000_000);
            }
            other => panic!("expected Pong, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stats_request_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(&mut client, &Request::Stats).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Request, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Request::Stats)));
    }

    #[tokio::test]
    async fn stats_response_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        let response = Response::Stats {
            cpu_percent: 12.5,
            memory_bytes: 1_267_296,
        };
        write_frame(&mut server, &response).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut client);
        let received = read_frame::<Response, _>(&mut reader).await.unwrap();
        match received {
            Some(Response::Stats {
                cpu_percent,
                memory_bytes,
            }) => {
                assert_eq!(cpu_percent, 12.5);
                assert_eq!(memory_bytes, 1_267_296);
            }
            other => panic!("expected Stats, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn event_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(
            &mut client,
            &Event::LogLine {
                line: "hello".to_string(),
            },
        )
        .await
        .unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Event, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Event::LogLine { line }) if line == "hello"));
    }

    #[tokio::test]
    async fn read_frame_returns_none_on_clean_eof() {
        let (client, mut server) = tokio::io::duplex(1024);
        drop(client);
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Request, _>(&mut reader).await.unwrap();
        assert!(received.is_none());
    }
}
