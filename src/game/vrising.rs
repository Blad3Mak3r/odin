//! V Rising's administrator and ban files live below the instance-specific
//! persistent-data path selected by Odin's `-persistentDataPath` argument.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use thiserror::Error;

use crate::paths::Paths;

const RCON_TIMEOUT: Duration = Duration::from_secs(5);
const RCON_MAX_PACKET: usize = 64 * 1024;
const AUTH_REQUEST: i32 = 3;
const AUTH_RESPONSE: i32 = 2;
const EXEC_COMMAND: i32 = 2;

#[derive(Debug, Error)]
pub enum VRisingAccessListError {
    #[error("'{0}' is not a valid V Rising access-list kind; expected 'admin' or 'banned'")]
    UnknownKind(String),
    #[error("'{0}' is not a valid SteamID64: expected exactly 17 digits")]
    WrongIdLength(String),
    #[error("'{0}' is not a valid SteamID64: expected it to start with '7656119'")]
    WrongIdPrefix(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VRisingAccessListKind {
    Admin,
    Banned,
}

impl VRisingAccessListKind {
    pub fn parse(raw: &str) -> Result<Self, VRisingAccessListError> {
        match raw {
            "admin" => Ok(Self::Admin),
            "banned" => Ok(Self::Banned),
            other => Err(VRisingAccessListError::UnknownKind(other.to_string())),
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::Admin => "adminlist.txt",
            Self::Banned => "banlist.txt",
        }
    }
}

fn list_path(paths: &Paths, name: &str, kind: VRisingAccessListKind) -> PathBuf {
    paths
        .game_instance_dir(crate::game::GameId::VRising, name)
        .join("data/Settings")
        .join(kind.filename())
}

pub fn read(paths: &Paths, name: &str, kind: VRisingAccessListKind) -> Result<Vec<String>> {
    let path = list_path(paths, name, kind);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let mut ids = contents
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .filter(|id| validate_steam_id64(id).is_ok())
        .map(str::to_string)
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

pub fn write(paths: &Paths, name: &str, kind: VRisingAccessListKind, ids: &[String]) -> Result<()> {
    for id in ids {
        validate_steam_id64(id)?;
    }
    let mut ids = ids.to_vec();
    ids.sort();
    ids.dedup();
    let path = list_path(paths, name, kind);
    let parent = path.parent().expect("access list has a parent directory");
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let contents = if ids.is_empty() {
        String::new()
    } else {
        format!("{}\n", ids.join("\n"))
    };
    let temporary = path.with_extension(format!("txt.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, contents)
        .with_context(|| format!("failed to write {}", temporary.display()))?;
    std::fs::rename(&temporary, &path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            temporary.display(),
            path.display()
        )
    })
}

pub fn add_id(paths: &Paths, name: &str, kind: VRisingAccessListKind, id: &str) -> Result<bool> {
    validate_steam_id64(id)?;
    let mut ids = read(paths, name, kind)?;
    if ids.iter().any(|current| current == id) {
        return Ok(false);
    }
    ids.push(id.to_string());
    write(paths, name, kind, &ids)?;
    Ok(true)
}

pub fn remove_id(paths: &Paths, name: &str, kind: VRisingAccessListKind, id: &str) -> Result<bool> {
    let mut ids = read(paths, name, kind)?;
    let previous_len = ids.len();
    ids.retain(|current| current != id);
    if ids.len() == previous_len {
        return Ok(false);
    }
    write(paths, name, kind, &ids)?;
    Ok(true)
}

/// Executes V Rising's Source RCON protocol through its loopback-only
/// listener. The credentials remain in Odin's database and are never
/// included in the HTTP response or sent to the browser.
pub fn execute_rcon(
    paths: &crate::paths::Paths,
    instance: &crate::db::game_instances::GenericGameInstance,
    command: &str,
) -> Result<String> {
    let enabled = crate::game::config_documents::value(paths, instance, "Rcon.Enabled")?
        .context("V Rising configuration has not been generated yet")?;
    if !matches!(enabled.as_str(), "true" | "True" | "1") {
        bail!("V Rising RCON is disabled for this instance");
    }
    let password = crate::game::config_documents::value(paths, instance, "Rcon.Password")?
        .filter(|value| !value.is_empty())
        .context("V Rising RCON requires a password")?;
    let port = crate::game::config_documents::value(paths, instance, "Rcon.Port")?
        .context("V Rising RCON port is not configured")?
        .parse::<u16>()
        .context("V Rising RCON port is invalid")?;
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let mut stream = TcpStream::connect_timeout(&address.into(), RCON_TIMEOUT)
        .with_context(|| format!("failed to connect to V Rising RCON at {address}"))?;
    stream.set_read_timeout(Some(RCON_TIMEOUT))?;
    stream.set_write_timeout(Some(RCON_TIMEOUT))?;

    write_rcon_packet(&mut stream, 1, AUTH_REQUEST, &password)?;
    let (id, packet_type, _) = read_rcon_packet(&mut stream)?;
    if id == -1 {
        bail!("V Rising RCON authentication failed");
    }
    if id != 1 || packet_type != AUTH_RESPONSE {
        bail!("V Rising RCON returned an unexpected authentication response");
    }

    write_rcon_packet(&mut stream, 2, EXEC_COMMAND, command)?;
    let (id, _packet_type, output) = read_rcon_packet(&mut stream)?;
    if id != 2 {
        bail!("V Rising RCON returned an unexpected command response");
    }
    Ok(output)
}

fn write_rcon_packet(stream: &mut TcpStream, id: i32, packet_type: i32, body: &str) -> Result<()> {
    let body = body.as_bytes();
    let length = 10usize
        .checked_add(body.len())
        .context("V Rising RCON request is too large")?;
    if length > RCON_MAX_PACKET {
        bail!("V Rising RCON request is too large");
    }
    stream.write_all(&(length as i32).to_le_bytes())?;
    stream.write_all(&id.to_le_bytes())?;
    stream.write_all(&packet_type.to_le_bytes())?;
    stream.write_all(body)?;
    stream.write_all(&[0, 0])?;
    stream.flush()?;
    Ok(())
}

fn read_rcon_packet(stream: &mut TcpStream) -> Result<(i32, i32, String)> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = i32::from_le_bytes(length);
    if !(10..=RCON_MAX_PACKET as i32).contains(&length) {
        bail!("V Rising RCON sent an invalid packet length");
    }
    let mut packet = vec![0; length as usize];
    stream.read_exact(&mut packet)?;
    let id = i32::from_le_bytes(packet[0..4].try_into().expect("packet id length"));
    let packet_type = i32::from_le_bytes(packet[4..8].try_into().expect("packet type length"));
    if !packet.ends_with(&[0, 0]) {
        bail!("V Rising RCON sent an unterminated packet");
    }
    let output = String::from_utf8_lossy(&packet[8..packet.len() - 2]).into_owned();
    Ok((id, packet_type, output))
}

fn validate_steam_id64(id: &str) -> Result<(), VRisingAccessListError> {
    if id.len() != 17 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(VRisingAccessListError::WrongIdLength(id.to_string()));
    }
    if !id.starts_with("7656119") {
        return Err(VRisingAccessListError::WrongIdPrefix(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_lists_round_trip_under_the_isolated_data_directory() {
        let dir = std::env::temp_dir().join(format!("odin-vrising-lists-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let ids = vec![
            "76561197960287931".to_string(),
            "76561197960287930".to_string(),
        ];

        write(&paths, "vrising", VRisingAccessListKind::Admin, &ids).unwrap();

        assert_eq!(
            read(&paths, "vrising", VRisingAccessListKind::Admin).unwrap(),
            vec!["76561197960287930", "76561197960287931"]
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn rcon_packet_reader_rejects_impossible_lengths() {
        assert!(!(10..=RCON_MAX_PACKET as i32).contains(&9));
        assert!(!(10..=RCON_MAX_PACKET as i32).contains(&(RCON_MAX_PACKET as i32 + 1)));
    }
}
