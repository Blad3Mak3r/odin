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

use crate::db::game_instances::GameInstanceIdentity;
use crate::game::{GameId, SEVEN_DAYS_TEMPLATE_BASELINE_FILE};
use crate::paths::Paths;

/// The launch arguments Odin passes to the server itself. Every other setting
/// lives in the game's own `serverconfig.xml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SevenDaysLaunch {
    pub name: String,
    pub port: u16,
    pub telnet_port: Option<u16>,
}

impl From<&crate::db::game_instances::GenericGameInstance> for SevenDaysLaunch {
    fn from(instance: &crate::db::game_instances::GenericGameInstance) -> Self {
        Self {
            name: instance.name().to_string(),
            port: instance.config.port,
            telnet_port: instance.config.admin_port,
        }
    }
}

/// The single definition of the dedicated server's launch contract. The
/// instance directory holds the native `config/serverconfig.xml`, the saves,
/// the mods and the logs, so it doubles as the game's user data folder.
pub fn build_command_at(
    install_dir: &Path,
    instance_dir: &Path,
    console_log: &Path,
    launch: &SevenDaysLaunch,
) -> Result<tokio::process::Command> {
    let binary = install_dir.join(crate::game::driver(GameId::SevenDaysToDie).server_binary());
    if !binary.is_file() {
        bail!(
            "7 Days to Die is not installed (expected {})",
            binary.display()
        );
    }
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(console_log)
        .with_context(|| format!("failed to open {}", console_log.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to duplicate 7 Days to Die log handle")?;
    let mut command = tokio::process::Command::new(binary);
    command
        .current_dir(install_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr))
        .process_group(0)
        .arg(format!(
            "-configfile={}",
            instance_dir.join(CONFIG_FILE).display()
        ))
        .arg(format!("-UserDataFolder={}", instance_dir.display()))
        .arg(format!("-ServerPort={}", launch.port))
        // Unity writes this log itself; pointing it at stdout sends it
        // through the runtime, which stamps and cycles `console.log`.
        .arg("-logfile=/dev/stdout")
        .arg("-quit")
        .arg("-batchmode")
        .arg("-nographics");
    if let Some(port) = launch.telnet_port {
        command.arg(format!("-TelnetPort={port}"));
    }
    // The dedicated flag must stay last.
    command.arg("-dedicated");
    Ok(command)
}

/// Where the installed server's `serverconfig.xml` template lives for an
/// instance: its private copy of the game once installed, the shared
/// installation for an instance that uses it (which has no private copy), and
/// otherwise a template placed in the host install directory.
pub fn template_path(paths: &Paths, identity: &GameInstanceIdentity) -> PathBuf {
    let private = paths
        .container_runtime_dir(GameId::SevenDaysToDie, &identity.id)
        .join(GameId::SevenDaysToDie.as_str())
        .join(TEMPLATE_FILE);
    if private.is_file() {
        return private;
    }
    let shared = paths
        .shared_install_dir(GameId::SevenDaysToDie)
        .join(TEMPLATE_FILE);
    if shared.is_file() {
        return shared;
    }
    paths
        .game_install_dir(GameId::SevenDaysToDie)
        .join(TEMPLATE_FILE)
}

/// Creates the native document from the template the installed server ships,
/// plus the baseline the template review compares against. An existing file
/// is never touched.
pub fn seed_config(template: &Path, instance_dir: &Path) -> Result<()> {
    let destination = instance_dir.join(CONFIG_FILE);
    if destination.exists() {
        return Ok(());
    }
    if !template.is_file() {
        bail!(
            "7 Days to Die configuration template not found at {}",
            template.display()
        );
    }
    let parent = destination
        .parent()
        .context("7 Days to Die configuration path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    fs::copy(template, &destination).with_context(|| {
        format!(
            "failed to copy 7 Days to Die configuration from {}",
            template.display()
        )
    })?;
    fs::copy(template, parent.join(SEVEN_DAYS_TEMPLATE_BASELINE_FILE))
        .context("failed to save the 7 Days to Die configuration template")?;
    Ok(())
}

/// Container-side preparation, run before every launch: the game is installed
/// in the private runtime volume by now, so its template can seed a missing
/// configuration.
pub fn prepare_runtime(install_dir: &Path, instance_dir: &Path) -> Result<()> {
    seed_config(&install_dir.join(TEMPLATE_FILE), instance_dir)
}

const TEMPLATE_FILE: &str = "serverconfig.xml";

/// Relative to the instance directory.
pub const CONFIG_FILE: &str = "config/serverconfig.xml";
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
    paths: &Paths,
    instance: &crate::db::game_instances::GenericGameInstance,
    command: &str,
) -> Result<ConsoleResponse> {
    let console =
        Console::from_config(|key| crate::game::config_documents::value(paths, instance, key))?;
    console.execute(command)
}

/// Asks a running server to save its world and exit, using the console
/// settings in the instance's own `serverconfig.xml`.
pub fn shutdown(instance_dir: &Path) -> Result<()> {
    let config = instance_dir.join(CONFIG_FILE);
    Console::from_config(|key| crate::game::config_documents::xml_value(&config, key))?
        .execute("shutdown")
        .map(|_| ())
}

struct Console {
    port: u16,
    password: String,
}

impl Console {
    fn from_config(read: impl Fn(&str) -> Result<Option<String>>) -> Result<Self> {
        let enabled = read("TelnetEnabled")?
            .context("7 Days to Die configuration has not been generated yet")?;
        if !matches!(enabled.as_str(), "true" | "True" | "1") {
            bail!("7 Days to Die console is disabled for this instance");
        }
        let password = read("TelnetPassword")?
            .filter(|value| !value.is_empty())
            .context("7 Days to Die console requires a password")?;
        let port = read("TelnetPort")?
            .context("7 Days to Die console port is not configured")?
            .parse::<u16>()
            .context("7 Days to Die console port is invalid")?;
        Ok(Self { port, password })
    }

    fn execute(&self, command: &str) -> Result<ConsoleResponse> {
        let (port, password) = (self.port, self.password.as_str());
        let command = command.trim();
        if command.is_empty() || command.len() > 16 * 1024 || command.contains(['\r', '\n']) {
            bail!("console command must be one non-empty line no longer than 16 KiB");
        }
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let mut stream = TcpStream::connect_timeout(&address.into(), CONSOLE_TIMEOUT)
            .with_context(|| format!("failed to connect to 7 Days to Die console at {address}"))?;
        stream.set_write_timeout(Some(CONSOLE_TIMEOUT))?;
        // The game uses a line-oriented raw TCP service, despite its Telnet name.
        // It prints a greeting first, then accepts the configured password.
        let _ = read_available(&mut stream, Duration::from_secs(1))?;
        stream.write_all(password.as_bytes())?;
        stream.write_all(b"\n")?;
        let authentication = read_available(&mut stream, Duration::from_secs(2))?;
        if authentication.to_ascii_lowercase().contains("incorrect")
            || authentication.to_ascii_lowercase().contains("failed")
        {
            bail!("7 Days to Die console authentication failed");
        }
        stream.write_all(command.as_bytes())?;
        stream.write_all(b"\n")?;
        let output = read_available(&mut stream, Duration::from_secs(2))?;
        Ok(ConsoleResponse { output })
    }
}

fn read_available(stream: &mut TcpStream, first_byte_timeout: Duration) -> Result<String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    stream.set_read_timeout(Some(first_byte_timeout))?;
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                bytes.extend_from_slice(&buffer[..read]);
                if bytes.len() > MAX_CONSOLE_RESPONSE {
                    bail!("7 Days to Die console response exceeds 256 KiB");
                }
                // The protocol has no explicit response framing. Once data
                // arrives, a brief idle period marks the end of this command.
                stream.set_read_timeout(Some(Duration::from_millis(150)))?;
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
    validate_mod_name(&name)?;
    Ok(ModInfo {
        name,
        display_name: value(xml, "DisplayName").context("ModInfo.xml is missing DisplayName")?,
        version: value(xml, "Version").context("ModInfo.xml is missing Version")?,
        description: value(xml, "Description"),
        author: value(xml, "Author"),
        website: value(xml, "Website"),
    })
}

fn validate_mod_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        bail!("mod name may only contain letters, numbers, '_' and '-'");
    }
    Ok(())
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

/// Removes an instance-local 7 Days to Die mod after validating its directory
/// name so a request cannot escape the instance's `Mods` directory.
pub fn remove(paths: &Paths, db: &crate::db::Db, instance: &str, mod_name: &str) -> Result<()> {
    if crate::game::instances::is_running(paths, db, GameId::SevenDaysToDie, instance)? {
        return Err(crate::instance::InstanceError::ModsLocked(instance.to_owned()).into());
    }
    validate_mod_name(mod_name)?;
    let target = mods_dir(paths, instance).join(mod_name);
    if !target.is_dir() {
        bail!("mod '{mod_name}' is not installed");
    }
    fs::remove_dir_all(target).with_context(|| format!("failed to remove mod '{mod_name}'"))
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

    #[test]
    fn launch_command_is_the_complete_server_contract() {
        let root = std::env::temp_dir().join(format!("odin-7d2d-command-{}", uuid::Uuid::new_v4()));
        let install = root.join("install");
        let instance = root.join("instance");
        fs::create_dir_all(&install).unwrap();
        fs::create_dir_all(instance.join("logs")).unwrap();
        fs::write(install.join("7DaysToDieServer.x86_64"), "").unwrap();
        let log = instance.join("logs/console.log");
        let launch = SevenDaysLaunch {
            name: "undead".into(),
            port: 26900,
            telnet_port: Some(26903),
        };

        let command = build_command_at(&install, &instance, &log, &launch).unwrap();
        let command = command.as_std();
        assert_eq!(
            command.get_program(),
            install.join("7DaysToDieServer.x86_64")
        );
        assert_eq!(command.get_current_dir(), Some(install.as_path()));
        let arguments: Vec<_> = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            arguments,
            [
                format!(
                    "-configfile={}",
                    instance.join("config/serverconfig.xml").display()
                ),
                format!("-UserDataFolder={}", instance.display()),
                "-ServerPort=26900".to_string(),
                "-logfile=/dev/stdout".to_string(),
                "-quit".to_string(),
                "-batchmode".to_string(),
                "-nographics".to_string(),
                "-TelnetPort=26903".to_string(),
                "-dedicated".to_string(),
            ]
        );
        // The native document is never created by building the command.
        assert!(!instance.join("config/serverconfig.xml").exists());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn seeding_copies_the_template_and_a_baseline_only_when_the_config_is_missing() {
        let root = std::env::temp_dir().join(format!("odin-7d2d-seed-{}", uuid::Uuid::new_v4()));
        let install = root.join("install");
        let instance = root.join("instance");
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("serverconfig.xml"), "<ServerSettings/>").unwrap();

        prepare_runtime(&install, &instance).unwrap();
        assert_eq!(
            fs::read_to_string(instance.join("config/serverconfig.xml")).unwrap(),
            "<ServerSettings/>"
        );
        assert_eq!(
            fs::read_to_string(
                instance
                    .join("config")
                    .join(SEVEN_DAYS_TEMPLATE_BASELINE_FILE)
            )
            .unwrap(),
            "<ServerSettings/>"
        );

        // An operator's edits survive a later start, even after the game
        // ships a new template.
        fs::write(instance.join("config/serverconfig.xml"), "<edited/>").unwrap();
        fs::write(
            install.join("serverconfig.xml"),
            "<ServerSettings v=\"2\"/>",
        )
        .unwrap();
        prepare_runtime(&install, &instance).unwrap();
        assert_eq!(
            fs::read_to_string(instance.join("config/serverconfig.xml")).unwrap(),
            "<edited/>"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn seeding_without_a_template_fails_only_when_a_config_is_needed() {
        let root =
            std::env::temp_dir().join(format!("odin-7d2d-notemplate-{}", uuid::Uuid::new_v4()));
        let instance = root.join("instance");
        let error = prepare_runtime(&root.join("install"), &instance).unwrap_err();
        assert!(error.to_string().contains("template not found"));

        fs::create_dir_all(instance.join("config")).unwrap();
        fs::write(instance.join("config/serverconfig.xml"), "<edited/>").unwrap();
        prepare_runtime(&root.join("install"), &instance).unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn launch_command_requires_an_installed_server() {
        let root = std::env::temp_dir().join(format!("odin-7d2d-missing-{}", uuid::Uuid::new_v4()));
        let launch = SevenDaysLaunch {
            name: "undead".into(),
            port: 26900,
            telnet_port: None,
        };
        let error = build_command_at(&root, &root, &root.join("console.log"), &launch).unwrap_err();
        assert!(error.to_string().contains("not installed"));
    }

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
        let template = paths
            .game_install_dir(GameId::SevenDaysToDie)
            .join("serverconfig.xml");
        fs::create_dir_all(template.parent().unwrap()).unwrap();
        fs::write(template, "<ServerSettings/>").unwrap();
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

    #[test]
    fn removes_an_installed_mod_without_allowing_path_traversal() {
        let paths = temporary_paths("remove");
        let db = crate::db::Db::open(&paths).unwrap();
        let template = paths
            .game_install_dir(GameId::SevenDaysToDie)
            .join("serverconfig.xml");
        fs::create_dir_all(template.parent().unwrap()).unwrap();
        fs::write(template, "<ServerSettings/>").unwrap();
        crate::db::game_instances::create_generic(&paths, &db, GameId::SevenDaysToDie, "undead")
            .unwrap();
        let installed = paths
            .game_instance_dir(GameId::SevenDaysToDie, "undead")
            .join("Mods/Example_Mod");
        fs::create_dir_all(&installed).unwrap();

        remove(&paths, &db, "undead", "Example_Mod").unwrap();
        assert!(!installed.exists());
        assert!(remove(&paths, &db, "undead", "../outside").is_err());

        drop(db);
        fs::remove_dir_all(paths.data_dir).ok();
    }
}
