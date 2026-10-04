//! Rust WebRCON client used by the dashboard's per-instance console.
//!
//! Rust Dedicated exposes its RCON endpoint as a WebSocket when launched with
//! `+rcon.web 1`. Odin always enables that mode and only connects back to the
//! loopback address, so its RCON port never has to be reachable by a browser.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use crate::db::game_instances::RustInstance;
use crate::instance::InstanceError;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_IDENTIFIER: u64 = 1;

#[derive(Deserialize)]
struct RconResponse {
    #[serde(rename = "Identifier")]
    identifier: u64,
    #[serde(rename = "Message")]
    message: String,
}

/// Runs one console command through the instance's loopback-only WebRCON
/// connection and returns the matching server response.
pub async fn execute(instance: &RustInstance, command: &str) -> Result<String> {
    if !instance.is_running() {
        bail!(InstanceError::NotRunning(instance.name().to_owned()));
    }

    let endpoint = websocket_url(instance.config.rcon_port, &instance.config.rcon_password);
    let (mut socket, _) = timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(endpoint))
        .await
        .context("timed out connecting to Rust WebRCON")?
        .context("failed to connect to Rust WebRCON")?;
    let request = serde_json::json!({
        "Identifier": REQUEST_IDENTIFIER,
        "Message": command,
        "Name": "WebRcon",
    });
    timeout(
        RESPONSE_TIMEOUT,
        socket.send(Message::Text(request.to_string().into())),
    )
    .await
    .context("timed out sending command to Rust WebRCON")?
    .context("failed to send command to Rust WebRCON")?;

    loop {
        let frame = timeout(RESPONSE_TIMEOUT, socket.next())
            .await
            .context("timed out waiting for a Rust WebRCON response")?
            .context("Rust WebRCON closed before responding")?
            .context("failed to read Rust WebRCON response")?;
        match frame {
            Message::Text(text) => {
                let response: RconResponse =
                    serde_json::from_str(&text).context("Rust WebRCON sent an invalid response")?;
                if response.identifier == REQUEST_IDENTIFIER {
                    return Ok(response.message);
                }
            }
            Message::Close(_) => bail!("Rust WebRCON closed before responding"),
            _ => {}
        }
    }
}

fn websocket_url(port: u16, password: &str) -> String {
    format!("ws://127.0.0.1:{port}/{}", encode_path_segment(password))
}

fn encode_path_segment(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_url_keeps_the_rcon_password_in_one_path_segment() {
        assert_eq!(
            websocket_url(28016, "secret /?#€"),
            "ws://127.0.0.1:28016/secret%20%2F%3F%23%E2%82%AC"
        );
    }
}
