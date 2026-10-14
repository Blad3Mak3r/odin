//! Valve's Source RCON protocol over TCP, used by ARK: Survival Ascended.
//!
//! The servers run on the host network, so this deliberately connects only to
//! the loopback listener Odin launched.

use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const RCON_TIMEOUT: Duration = Duration::from_secs(5);
const RCON_AUTH: i32 = 3;
const RCON_AUTH_RESPONSE: i32 = 2;
const RCON_COMMAND: i32 = 2;
const REQUEST_ID: i32 = 1;
const MAX_RCON_PACKET_SIZE: usize = 64 * 1024;

/// Authenticates, then delivers each command in order without waiting for a
/// reply: a command that stops the server closes the socket as it runs, so
/// waiting would report a successful shutdown as a failure.
pub async fn send(port: u16, password: &str, commands: &[&str]) -> Result<()> {
    let mut stream = timeout(RCON_TIMEOUT, TcpStream::connect(("127.0.0.1", port)))
        .await
        .context("timed out connecting to Source RCON")?
        .context("failed to connect to Source RCON")?;
    write_rcon_packet(&mut stream, REQUEST_ID, RCON_AUTH, password).await?;
    let (id, packet_type, _) = read_rcon_packet(&mut stream).await?;
    anyhow::ensure!(
        id == REQUEST_ID && packet_type == RCON_AUTH_RESPONSE,
        "Source RCON authentication failed"
    );
    for command in commands {
        write_rcon_packet(&mut stream, REQUEST_ID, RCON_COMMAND, command).await?;
    }
    Ok(())
}

async fn write_rcon_packet(
    stream: &mut TcpStream,
    id: i32,
    packet_type: i32,
    body: &str,
) -> Result<()> {
    let size = 8usize
        .checked_add(body.len())
        .and_then(|value| value.checked_add(2))
        .context("Source RCON request is too large")?;
    let size = i32::try_from(size).context("Source RCON request is too large")?;
    timeout(RCON_TIMEOUT, async {
        stream.write_i32_le(size).await?;
        stream.write_i32_le(id).await?;
        stream.write_i32_le(packet_type).await?;
        stream.write_all(body.as_bytes()).await?;
        stream.write_all(&[0, 0]).await?;
        stream.flush().await
    })
    .await
    .context("timed out writing Source RCON request")?
    .context("failed to write Source RCON request")?;
    Ok(())
}

async fn read_rcon_packet(stream: &mut TcpStream) -> Result<(i32, i32, String)> {
    let packet = timeout(RCON_TIMEOUT, async {
        let size = stream.read_i32_le().await?;
        anyhow::ensure!(
            size >= 10 && usize::try_from(size).unwrap_or_default() <= MAX_RCON_PACKET_SIZE,
            "Source RCON sent an invalid packet size"
        );
        let mut payload = vec![0; usize::try_from(size).expect("validated positive packet size")];
        stream.read_exact(&mut payload).await?;
        Ok::<_, anyhow::Error>(payload)
    })
    .await
    .context("timed out reading Source RCON response")??;
    let id = i32::from_le_bytes(packet[0..4].try_into().expect("packet includes id"));
    let packet_type = i32::from_le_bytes(packet[4..8].try_into().expect("packet includes type"));
    anyhow::ensure!(
        packet[packet.len() - 2..] == [0, 0],
        "Source RCON response is missing its terminator"
    );
    let body = String::from_utf8(packet[8..packet.len() - 2].to_vec())
        .context("Source RCON response is not UTF-8")?;
    Ok((id, packet_type, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn source_rcon_packet_has_the_expected_framing() {
        let mut packet = Vec::new();
        packet.extend_from_slice(&16_i32.to_le_bytes());
        packet.extend_from_slice(&REQUEST_ID.to_le_bytes());
        packet.extend_from_slice(&RCON_COMMAND.to_le_bytes());
        packet.extend_from_slice(b"save");
        packet.extend_from_slice(&[0, 0]);
        assert_eq!(packet.len(), 18);
        assert_eq!(&packet[12..], b"save\0\0");
    }

    /// Reads one request as the server would and returns its type and body.
    async fn read_request(stream: &mut TcpStream) -> (i32, i32, String) {
        read_rcon_packet(stream).await.unwrap()
    }

    #[tokio::test]
    async fn send_authenticates_before_delivering_each_command_in_order() {
        let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind a loopback listener: {error}"),
        };
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let auth = read_request(&mut stream).await;
            write_rcon_packet(&mut stream, REQUEST_ID, RCON_AUTH_RESPONSE, "")
                .await
                .unwrap();
            let first = read_request(&mut stream).await;
            let second = read_request(&mut stream).await;
            (auth, first, second)
        });

        send(port, "secret", &["SaveWorld", "DoExit"])
            .await
            .unwrap();

        let (auth, first, second) = server.await.unwrap();
        assert_eq!(auth, (REQUEST_ID, RCON_AUTH, "secret".to_string()));
        assert_eq!(first, (REQUEST_ID, RCON_COMMAND, "SaveWorld".to_string()));
        assert_eq!(second, (REQUEST_ID, RCON_COMMAND, "DoExit".to_string()));
    }

    #[tokio::test]
    async fn send_stops_when_the_server_rejects_the_password() {
        let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind a loopback listener: {error}"),
        };
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            // Source servers answer a bad password with request id -1.
            write_rcon_packet(&mut stream, -1, RCON_AUTH_RESPONSE, "")
                .await
                .unwrap();
        });

        let error = send(port, "wrong", &["quit"]).await.unwrap_err();

        server.await.unwrap();
        assert!(error.to_string().contains("authentication failed"));
    }
}
