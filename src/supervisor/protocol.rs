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

use crate::player_events::PlayerInfo;

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
    /// Currently-connected players, as tracked by the supervisor's own
    /// `console.log` parsing (see `server`'s player tracker) — the latest
    /// state, not a live re-scan.
    Players,
    /// When the world was last saved, as recognized by the supervisor's own
    /// `console.log` parsing — `None` if it hasn't saved since this
    /// supervisor started.
    LastSaved,
    /// Diagnostics for the most recent exit of this supervisor's child
    /// (deliberate or not — including one already superseded by an
    /// in-place automatic restart), so a crash-loop is debuggable without
    /// digging through raw `console.log`. `None` if the current child
    /// hasn't exited yet.
    LastExit,
}

/// See `Request::LastExit`/`Response::LastExit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastExitInfo {
    pub code: Option<i32>,
    pub at: DateTime<Utc>,
    /// The tail of `console.log` right before this exit — bounded by
    /// `server::RECENT_LINES_CAPACITY`, oldest first.
    pub recent_lines: Vec<String>,
}

/// The reply to a `Request`, over the same control-socket connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong {
        pid: u32,
        pid_started_at: i64,
        started_at: DateTime<Utc>,
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
        ready: bool,
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
    /// The currently-connected player list — see `Request::Players`.
    Players {
        players: Vec<PlayerInfo>,
    },
    /// The last-save timestamp — see `Request::LastSaved`.
    LastSaved {
        at: Option<DateTime<Utc>>,
    },
    /// Diagnostics for the most recent exit — see `Request::LastExit`.
    LastExit {
        info: Option<LastExitInfo>,
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
    LogLine {
        line: String,
    },
    /// Pushed in addition to the `LogLine` carrying the same source line,
    /// whenever the supervisor's own parsing recognizes a join.
    PlayerJoined {
        name: String,
    },
    /// The `PlayerJoined` counterpart, pushed on a recognized leave.
    PlayerLeft {
        name: String,
    },
    /// The supervisor respawned its child in place after an unexpected
    /// exit (the instance has automatic restart enabled) instead of
    /// exiting itself — no fields, since a subscriber already re-reads the
    /// new pid from the next telemetry tick; this exists purely so
    /// `odin serve` can record the same `InstanceAutoRestarted` activity it
    /// used to detect on its own (much later) via polling.
    Restarted,
    /// Pushed in addition to the `LogLine` carrying the same source line,
    /// whenever the supervisor's own parsing recognizes a save completing.
    WorldSaved {
        at: DateTime<Utc>,
    },
    Exited {
        code: Option<i32>,
    },
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
            r#"{"type":"pong","pid":4242,"pid_started_at":1700000000,"started_at":"2026-08-29T00:00:00Z","ready":true}"#,
        )
        .unwrap();

        assert!(matches!(
            response,
            Response::Pong {
                odin_version: None,
                ..
            }
        ));
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
    async fn players_request_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(&mut client, &Request::Players).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Request, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Request::Players)));
    }

    #[tokio::test]
    async fn players_response_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        let response = Response::Players {
            players: vec![PlayerInfo {
                name: "Bjorn".to_string(),
                connected_at: Utc::now(),
            }],
        };
        write_frame(&mut server, &response).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut client);
        let received = read_frame::<Response, _>(&mut reader).await.unwrap();
        match received {
            Some(Response::Players { players }) => {
                assert_eq!(players.len(), 1);
                assert_eq!(players[0].name, "Bjorn");
            }
            other => panic!("expected Players, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn player_joined_event_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(
            &mut client,
            &Event::PlayerJoined {
                name: "Bjorn".to_string(),
            },
        )
        .await
        .unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Event, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Event::PlayerJoined { name }) if name == "Bjorn"));
    }

    #[tokio::test]
    async fn player_left_event_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(
            &mut client,
            &Event::PlayerLeft {
                name: "Bjorn".to_string(),
            },
        )
        .await
        .unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Event, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Event::PlayerLeft { name }) if name == "Bjorn"));
    }

    #[tokio::test]
    async fn restarted_event_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(&mut client, &Event::Restarted).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Event, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Event::Restarted)));
    }

    #[tokio::test]
    async fn last_saved_request_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(&mut client, &Request::LastSaved).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Request, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Request::LastSaved)));
    }

    #[tokio::test]
    async fn last_saved_response_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        let at = Utc::now();
        write_frame(&mut server, &Response::LastSaved { at: Some(at) })
            .await
            .unwrap();
        let mut reader = tokio::io::BufReader::new(&mut client);
        let received = read_frame::<Response, _>(&mut reader).await.unwrap();
        match received {
            Some(Response::LastSaved { at: Some(got) }) => assert_eq!(got, at),
            other => panic!("expected LastSaved, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn last_exit_request_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        write_frame(&mut client, &Request::LastExit).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Request, _>(&mut reader).await.unwrap();
        assert!(matches!(received, Some(Request::LastExit)));
    }

    #[tokio::test]
    async fn last_exit_response_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        let at = Utc::now();
        let response = Response::LastExit {
            info: Some(LastExitInfo {
                code: Some(1),
                at,
                recent_lines: vec!["boom".to_string()],
            }),
        };
        write_frame(&mut server, &response).await.unwrap();
        let mut reader = tokio::io::BufReader::new(&mut client);
        let received = read_frame::<Response, _>(&mut reader).await.unwrap();
        match received {
            Some(Response::LastExit {
                info:
                    Some(LastExitInfo {
                        code: Some(1),
                        at: got_at,
                        recent_lines,
                    }),
            }) => {
                assert_eq!(got_at, at);
                assert_eq!(recent_lines, vec!["boom".to_string()]);
            }
            other => panic!("expected LastExit, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn world_saved_event_round_trips_through_a_duplex_stream() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        let at = Utc::now();
        write_frame(&mut client, &Event::WorldSaved { at })
            .await
            .unwrap();
        let mut reader = tokio::io::BufReader::new(&mut server);
        let received = read_frame::<Event, _>(&mut reader).await.unwrap();
        match received {
            Some(Event::WorldSaved { at: got }) => assert_eq!(got, at),
            other => panic!("expected WorldSaved, got {other:?}"),
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
