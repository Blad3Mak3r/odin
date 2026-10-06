//! Instance-local 7 Days to Die mod archives.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::game::GameId;
use crate::paths::Paths;

pub const MAX_ARCHIVE_ENTRIES: usize = 10_000;
pub const MAX_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const CONSOLE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONSOLE_RESPONSE: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ConsoleResponse {
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub entity_id: String,
    pub name: String,
    pub platform_id: Option<String>,
}

/// Executes one 7D2D console command through the instance's password-protected
/// Telnet-compatible raw TCP endpoint. The dashboard never receives the
/// password, and Odin always connects to loopback; operators must still keep
/// the game port protected by their host firewall because the game itself does
/// not expose a bind-address setting for this endpoint.
pub fn execute_console(
    instance: &crate::db::game_instances::GenericGameInstance,
    command: &str,
) -> Result<ConsoleResponse> {
    let command = command.trim();
    if command.is_empty() || command.len() > 16 * 1024 || command.contains(['\r', '\n']) {
        bail!("console command must be one non-empty line no longer than 16 KiB");
    }
    if !instance
        .config
        .settings
        .get("telnet_enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        bail!("7 Days to Die console is disabled for this instance");
    }
    let password = instance
        .config
        .settings
        .get("telnet_password")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .context("7 Days to Die console requires a password")?;
    let port = instance
        .config
        .admin_port
        .context("7 Days to Die console port is not configured")?;
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let mut stream = TcpStream::connect_timeout(&address.into(), CONSOLE_TIMEOUT)
        .with_context(|| format!("failed to connect to 7 Days to Die console at {address}"))?;
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    stream.set_write_timeout(Some(CONSOLE_TIMEOUT))?;
    // The game uses a line-oriented raw TCP service, despite its Telnet name.
    // It prints a greeting first, then accepts the configured password.
    let _ = read_available(&mut stream)?;
    stream.write_all(password.as_bytes())?;
    stream.write_all(b"\n")?;
    let authentication = read_available(&mut stream)?;
    if authentication.to_ascii_lowercase().contains("incorrect")
        || authentication.to_ascii_lowercase().contains("failed")
    {
        bail!("7 Days to Die console authentication failed");
    }
    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")?;
    let output = read_available(&mut stream)?;
    Ok(ConsoleResponse { output })
}

fn read_available(stream: &mut TcpStream) -> Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                bytes.extend_from_slice(&buffer[..read]);
                if bytes.len() > MAX_CONSOLE_RESPONSE {
                    bail!("7 Days to Die console response exceeds 256 KiB");
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

/// Best-effort parser for `lpi` / `listplayerids`. The server has changed the
/// surrounding headings over releases, but each player row begins with an
/// entity id and contains the displayed name and optional platform id.
pub fn parse_players(output: &str) -> Vec<Player> {
    let expression = Regex::new(
        r"(?m)^\s*(\d+)\s*[,|:\t]\s*([^,|\t]+?)(?:\s*[,|:\t]\s*([A-Za-z0-9_:-]{8,}))?\s*$",
    )
    .expect("player expression is valid");
    expression
        .captures_iter(output)
        .map(|capture| Player {
            entity_id: capture[1].trim().into(),
            name: capture[2].trim().into(),
            platform_id: capture.get(3).map(|value| value.as_str().trim().into()),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct ModInfo {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub website: Option<String>,
}

fn mods_dir(paths: &Paths, instance: &str) -> PathBuf {
    paths
        .game_instance_dir(GameId::SevenDaysToDie, instance)
        .join("Mods")
}

fn value(xml: &str, tag: &str) -> Option<String> {
    let pattern = Regex::new(&format!(
        r#"(?is)<{}\s+[^>]*\bvalue\s*=\s*[\"']([^\"']*)[\"'][^>]*/?>"#,
        regex::escape(tag)
    ))
    .ok()?;
    pattern
        .captures(xml)
        .map(|captures| captures[1].trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn parse_mod_info(xml: &str) -> Result<ModInfo> {
    let name = value(xml, "Name").context("ModInfo.xml is missing Name")?;
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        bail!("ModInfo.xml Name may only contain letters, numbers, '_' and '-'");
    }
    Ok(ModInfo {
        name,
        display_name: value(xml, "DisplayName").context("ModInfo.xml is missing DisplayName")?,
        version: value(xml, "Version").context("ModInfo.xml is missing Version")?,
        description: value(xml, "Description"),
        author: value(xml, "Author"),
        website: value(xml, "Website"),
    })
}

fn archive_root_and_info(zip_path: &Path) -> Result<(String, ModInfo)> {
    let file = fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).context("uploaded file is not a valid ZIP archive")?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        bail!("ZIP contains more than {MAX_ARCHIVE_ENTRIES} entries");
    }
    let mut roots = HashSet::new();
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut info = None;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let enclosed = entry
            .enclosed_name()
            .context("ZIP contains an unsafe path")?
            .to_path_buf();
        if enclosed.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            bail!("ZIP contains an unsafe path");
        }
        if !names.insert(enclosed.clone()) {
            bail!("ZIP contains duplicate paths");
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("ZIP contains a symbolic link");
        }
        total = total
            .checked_add(entry.size())
            .context("ZIP is too large")?;
        if total > MAX_UNCOMPRESSED_BYTES {
            bail!("ZIP expands beyond 2 GiB");
        }
        let mut components = enclosed.components();
        let Some(Component::Normal(root)) = components.next() else {
            bail!("ZIP entries must be inside one mod directory");
        };
        roots.insert(root.to_string_lossy().to_string());
        let rest = components.collect::<Vec<_>>();
        if rest.is_empty() && !entry.is_dir() {
            bail!("ZIP files must be inside one mod directory");
        }
        if rest.len() == 1
            && matches!(rest.first(), Some(Component::Normal(name)) if *name == "ModInfo.xml")
        {
            let mut xml = String::new();
            entry
                .read_to_string(&mut xml)
                .context("could not read ModInfo.xml")?;
            info = Some(parse_mod_info(&xml)?);
        }
    }
    if roots.len() != 1 {
        bail!("ZIP must contain exactly one mod directory");
    }
    Ok((
        roots.into_iter().next().expect("one root"),
        info.context("ZIP must contain ModInfo.xml directly inside its mod directory")?,
    ))
}

pub fn inspect_archive(zip_path: &Path) -> Result<ModInfo> {
    archive_root_and_info(zip_path).map(|(_, info)| info)
}

pub fn list(paths: &Paths, instance: &str) -> Result<Vec<ModInfo>> {
    let directory = mods_dir(paths, instance);
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut mods = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Ok(xml) = fs::read_to_string(entry.path().join("ModInfo.xml"))
            && let Ok(info) = parse_mod_info(&xml)
        {
            mods.push(info);
        }
    }
    mods.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(mods)
}

pub fn install(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &str,
    zip_path: &Path,
    replace: bool,
) -> Result<ModInfo> {
    if crate::game::instances::is_running(paths, db, GameId::SevenDaysToDie, instance)? {
        bail!("stop the 7 Days to Die instance before changing mods");
    }
    let (root, info) = archive_root_and_info(zip_path)?;
    let mods = mods_dir(paths, instance);
    fs::create_dir_all(&mods)?;
    let target = mods.join(&info.name);
    if target.exists() && !replace {
        bail!(
            "mod '{}' is already installed; retry with replace=true",
            info.name
        );
    }
    let staging = mods.join(format!(".odin-install-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&staging)?;
    let result = (|| -> Result<()> {
        let file = fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let relative = entry
                .enclosed_name()
                .context("ZIP contains an unsafe path")?
                .to_path_buf();
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                bail!("ZIP contains a symbolic link");
            }
            let destination = staging.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&destination)?;
            } else {
                let parent = destination.parent().context("ZIP path has no parent")?;
                fs::create_dir_all(parent)?;
                let mut output = fs::File::create(destination)?;
                std::io::copy(&mut entry, &mut output)?;
                output.flush()?;
            }
        }
        let extracted = staging.join(root);
        let extracted_info = parse_mod_info(&fs::read_to_string(extracted.join("ModInfo.xml"))?)?;
        if extracted_info.name != info.name {
            bail!("ModInfo.xml changed during extraction");
        }
        let backup = mods.join(format!(".odin-replaced-{}", uuid::Uuid::new_v4()));
        let had_previous = target.exists();
        if had_previous {
            fs::rename(&target, &backup)?;
        }
        if let Err(error) = fs::rename(&extracted, &target) {
            if had_previous {
                let _ = fs::rename(&backup, &target);
            }
            return Err(error.into());
        }
        if had_previous {
            fs::remove_dir_all(backup)?;
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    result?;
    Ok(info)
}

#[cfg(test)]
mod console_tests {
    use super::*;

    #[test]
    fn parses_player_rows_from_console_output() {
        let players =
            parse_players("EntityID, PlayerName, PlatformId\n42, Ada, EOS_abc12345\n77, Bob");
        assert_eq!(players.len(), 2);
        assert_eq!(players[0].entity_id, "42");
        assert_eq!(players[0].name, "Ada");
        assert_eq!(players[0].platform_id.as_deref(), Some("EOS_abc12345"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOD_INFO: &str = r#"<xml><Name value="Example_Mod"/><DisplayName value="Example Mod"/><Version value="1.2.3"/><Author value="Odin"/></xml>"#;

    fn temporary_paths(label: &str) -> Paths {
        let data_dir =
            std::env::temp_dir().join(format!("odin-7d2d-{label}-{}", uuid::Uuid::new_v4()));
        Paths {
            data_dir: data_dir.clone(),
            config_dir: data_dir,
        }
    }

    fn write_archive(path: &Path, entries: &[(&str, &str)]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents.as_bytes()).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn parses_required_v2_metadata() {
        let info = parse_mod_info(MOD_INFO).unwrap();
        assert_eq!(info.name, "Example_Mod");
        assert_eq!(info.display_name, "Example Mod");
        assert_eq!(info.version, "1.2.3");
    }

    #[test]
    fn rejects_unsafe_internal_name() {
        assert!(
            parse_mod_info(
                r#"<xml><Name value="../bad"/><DisplayName value="Bad"/><Version value="1"/></xml>"#
            )
            .is_err()
        );
    }

    #[test]
    fn inspects_single_root_mod_archive() {
        let path = std::env::temp_dir().join(format!("odin-7d2d-mod-{}.zip", uuid::Uuid::new_v4()));
        write_archive(&path, &[("Example_Mod/ModInfo.xml", MOD_INFO)]);
        assert_eq!(inspect_archive(&path).unwrap().name, "Example_Mod");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_archives_without_a_root_mod_info() {
        let path =
            std::env::temp_dir().join(format!("odin-7d2d-invalid-{}.zip", uuid::Uuid::new_v4()));
        write_archive(&path, &[("Example_Mod/Config/items.xml", "<items/>")]);
        assert!(inspect_archive(&path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_archives_with_multiple_mod_roots() {
        let path =
            std::env::temp_dir().join(format!("odin-7d2d-multiple-{}.zip", uuid::Uuid::new_v4()));
        write_archive(
            &path,
            &[
                ("First/ModInfo.xml", MOD_INFO),
                ("Second/ModInfo.xml", MOD_INFO),
            ],
        );
        assert!(inspect_archive(&path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn replacement_uses_internal_name_and_keeps_the_previous_mod_on_failure() {
        let paths = temporary_paths("replace");
        let db = crate::db::Db::open(&paths).unwrap();
        crate::db::game_instances::create_generic(&paths, &db, GameId::SevenDaysToDie, "undead")
            .unwrap();
        let first = paths.data_dir.join("first.zip");
        let second = paths.data_dir.join("second.zip");
        let invalid = paths.data_dir.join("invalid.zip");
        write_archive(
            &first,
            &[
                ("UnrelatedFolder/ModInfo.xml", MOD_INFO),
                ("UnrelatedFolder/payload.txt", "first"),
            ],
        );
        let version_two = MOD_INFO.replace("1.2.3", "2.0.0");
        write_archive(
            &second,
            &[
                ("DifferentArchiveName/ModInfo.xml", &version_two),
                ("DifferentArchiveName/payload.txt", "second"),
            ],
        );
        write_archive(&invalid, &[("broken/not-mod-info.xml", "<xml/>")]);

        install(&paths, &db, "undead", &first, false).unwrap();
        let target = paths
            .game_instance_dir(GameId::SevenDaysToDie, "undead")
            .join("Mods/Example_Mod");
        assert_eq!(
            fs::read_to_string(target.join("payload.txt")).unwrap(),
            "first"
        );

        assert!(install(&paths, &db, "undead", &second, false).is_err());
        assert_eq!(
            fs::read_to_string(target.join("payload.txt")).unwrap(),
            "first"
        );

        install(&paths, &db, "undead", &second, true).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("payload.txt")).unwrap(),
            "second"
        );
        assert!(install(&paths, &db, "undead", &invalid, true).is_err());
        assert_eq!(
            fs::read_to_string(target.join("payload.txt")).unwrap(),
            "second"
        );

        drop(db);
        fs::remove_dir_all(paths.data_dir).ok();
    }
}
