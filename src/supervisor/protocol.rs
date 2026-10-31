//! Wire protocol between `odin serve` (client, `super::client`) and the
//! `odin run` runtime inside each game container: newline-delimited JSON,
//! one message per line. Deliberately no heavier framing
//! (length-prefixing, a binary codec) — messages are small, and NDJSON is
//! trivially debuggable with `socat`/`nc` against a running instance's
//! socket, which matters more at this project's scale than shaving bytes.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt};

/// Current wire-protocol version. New fields must remain decodable by older
/// peers, and callers should use this value to decide whether an optional
/// operation is supported.
pub const PROTOCOL_VERSION: u16 = 2;

/// Maximum encoded frame size, including its trailing newline.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

fn default_ready() -> bool {
    // Supervisors predating readiness reporting were considered ready as soon
    // as their control socket answered.
    true
}

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
    /// Update the private game payload, keeping the Odin runtime container
    /// alive while its Valheim child is stopped and restarted.
    Update,
}

/// The reply to a `Request`, over the same control-socket connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong {
        pid: u32,
        pid_started_at: i64,
        started_at: DateTime<Utc>,
        /// Wire-protocol version implemented by the supervisor.
        #[serde(default)]
        protocol_version: u16,
        /// Odin version that owns this supervisor. `None` means the
        /// supervisor predates this protocol field and therefore needs a
        /// restart after Odin itself is upgraded.
        #[serde(default)]
        odin_version: Option<String>,
        /// Whether Valheim has finished loading the world and is actually
        /// accepting connections — not just "the process exists and this
        /// socket answers". `false` from the moment the child is spawned
        /// (or respawned, on automatic restart) until the supervisor's own
        /// `console.log` parsing recognizes it's come up.
        #[serde(default = "default_ready")]
        ready: bool,
    },
    Stopped,
    Updated,
    Error {
        #[serde(default)]
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    #[serde(other)]
    #[default]
    Unknown,
}

/// Reads one newline-delimited JSON message. `Ok(None)` means the peer
/// closed the connection cleanly (no partial trailing data) rather than
/// sending a final message — not an error.
pub async fn read_frame<T, R>(reader: &mut R) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::with_capacity(256);
    loop {
        let chunk = reader
            .fill_buf()
            .await
            .context("failed to read a frame from the supervisor socket")?;
        if chunk.is_empty() {
            break;
        }

        let (consumed, found_newline) = match chunk.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position + 1, true),
            None => (chunk.len(), false),
        };
        if frame.len() + consumed > MAX_FRAME_BYTES {
            anyhow::bail!("supervisor frame exceeds {MAX_FRAME_BYTES} bytes");
        }
        frame.extend_from_slice(&chunk[..consumed]);
        reader.consume(consumed);
        if found_newline {
            break;
        }
    }

    if frame.is_empty() {
        return Ok(None);
    }

    let line = std::str::from_utf8(&frame).context("supervisor frame is not UTF-8")?;
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
    if line.len() > MAX_FRAME_BYTES {
        anyhow::bail!("supervisor frame exceeds {MAX_FRAME_BYTES} bytes");
    }
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
            protocol_version: PROTOCOL_VERSION,
            odin_version: Some("0.7.0".to_string()),
            ready: true,
        };
        write_frame(&mut server, &response).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut client);
        let received = read_frame::<Response, _>(&mut reader).await.unwrap();
        match received {
            Some(Response::Pong {
                pid,
                pid_started_at,
                odin_version,
                ..
            }) => {
                assert_eq!(
                    (pid, pid_started_at, odin_version.as_deref()),
                    (4242, 1_700_000_000, Some("0.7.0"))
                );
            }
            other => panic!("expected Pong, got {other:?}"),
        }
    }

    #[test]
    fn pong_without_odin_version_decodes_as_an_old_supervisor() {
        let response: Response = serde_json::from_str(
            r#"{"type":"pong","pid":4242,"pid_started_at":1700000000,"started_at":"2026-08-29T00:00:00Z"}"#,
        )
        .unwrap();

        assert!(matches!(
            response,
            Response::Pong {
                odin_version: None,
                protocol_version: 0,
                ready: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn read_frame_returns_none_on_clean_eof() {
        let (client, mut server) = tokio::io::duplex(1024);
        drop(client);
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Request, _>(&mut reader).await.unwrap();
        assert!(received.is_none());
    }

    #[tokio::test]
    async fn read_frame_rejects_oversized_frames() {
        let (mut client, server) = tokio::io::duplex(1024);
        let payload = vec![b'x'; MAX_FRAME_BYTES + 1];
        let writer = tokio::spawn(async move {
            client.write_all(&payload).await.unwrap();
        });
        let mut reader = tokio::io::BufReader::new(server);
        let result = read_frame::<Request, _>(&mut reader).await;
        assert!(result.is_err());
        writer.await.unwrap();
    }
}
