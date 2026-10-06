//! Per-game configuration documents exposed by the dashboard.
//!
//! Odin owns a small set of lifecycle-critical settings (ports, names and
//! passwords).  Everything else stays in the game document and can be edited
//! as an advanced setting.  Keeping the mapping here makes the ownership
//! explicit instead of spreading stringly-typed file knowledge through routes.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::game_instances::GenericGameInstance;
use crate::game::GameId;
use crate::paths::Paths;

#[derive(Debug, Clone, Serialize)]
pub struct AdvancedConfigFile {
    pub id: String,
    pub path: String,
    pub format: &'static str,
    pub entries: Vec<AdvancedConfigEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvancedConfigEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdvancedConfigChange {
    pub file: String,
    pub key: String,
    pub value: String,
}

#[derive(Clone, Copy)]
enum Format {
    XmlProperties,
    Json,
    Ini,
    PalworldOptions,
}

#[derive(Clone, Copy)]
struct FileSpec {
    id: &'static str,
    path: &'static str,
    format: Format,
    managed: &'static [&'static str],
}

const SEVEN_DAYS_MANAGED: &[&str] = &[
    "ServerName",
    "ServerDescription",
    "ServerPassword",
    "ServerVisibility",
    "ServerMaxPlayerCount",
    "ServerPort",
    "GameWorld",
    "GameName",
    "WorldGenSeed",
    "WorldGenSize",
    "UserDataFolder",
    "SaveGameFolder",
    "TelnetEnabled",
    "ControlPanelEnabled",
    "WebDashboardEnabled",
    "SandboxCode",
];
const VRISING_MANAGED: &[&str] = &[
    "Name",
    "Port",
    "QueryPort",
    "MaxConnectedUsers",
    "Rcon.Enabled",
    "Rcon.Port",
    "Rcon.Password",
    "Rcon.BindAddress",
];
const PALWORLD_MANAGED: &[&str] = &[
    "ServerName",
    "ServerPlayerMaxNum",
    "PublicPort",
    "AdminPassword",
    "ServerPassword",
    "RESTAPIEnabled",
    "RESTAPIPort",
];
const DRAGONWILDS_MANAGED: &[&str] = &[
    "OwnerId",
    "ServerName",
    "DefaultWorldName",
    "AdminPassword",
    "DefaultWorldPassword",
];

fn specs(game: GameId) -> &'static [FileSpec] {
    match game {
        GameId::SevenDaysToDie => &[FileSpec {
            id: "server",
            path: "config/serverconfig.xml",
            format: Format::XmlProperties,
            managed: SEVEN_DAYS_MANAGED,
        }],
        GameId::VRising => &[FileSpec {
            id: "server-host",
            path: "data/Settings/ServerHostSettings.json",
            format: Format::Json,
            managed: VRISING_MANAGED,
        }],
        GameId::Palworld => &[FileSpec {
            id: "server",
            path: "runtime/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini",
            format: Format::PalworldOptions,
            managed: PALWORLD_MANAGED,
        }],
        GameId::RunescapeDragonwilds => &[FileSpec {
            id: "server",
            path: "runtime/RSDragonwilds/Saved/Config/Linux/DedicatedServer.ini",
            format: Format::Ini,
            managed: DRAGONWILDS_MANAGED,
        }],
        GameId::Valheim | GameId::Rust => &[],
    }
}

fn instance_root(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
    paths.game_instance_dir(instance.identity.game, instance.name())
}

fn document_path(paths: &Paths, instance: &GenericGameInstance, spec: FileSpec) -> PathBuf {
    instance_root(paths, instance).join(spec.path)
}

fn fallback(paths: &Paths, instance: &GenericGameInstance, spec: FileSpec) -> Result<String> {
    if instance.identity.game == GameId::SevenDaysToDie {
        let template = paths
            .game_install_dir(GameId::SevenDaysToDie)
            .join("serverconfig.xml");
        if template.is_file() {
            return fs::read_to_string(&template)
                .with_context(|| format!("failed to read {}", template.display()));
        }
        return Ok("<?xml version=\"1.0\"?>\n<ServerSettings>\n</ServerSettings>\n".into());
    }
    Ok(match spec.format {
        Format::Json => "{}\n".into(),
        Format::Ini | Format::PalworldOptions => String::new(),
        Format::XmlProperties => String::new(),
    })
}

fn read_document(paths: &Paths, instance: &GenericGameInstance, spec: FileSpec) -> Result<String> {
    let path = document_path(paths, instance, spec);
    if path.is_file() {
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
    } else {
        fallback(paths, instance, spec)
    }
}

pub fn list(paths: &Paths, instance: &GenericGameInstance) -> Result<Vec<AdvancedConfigFile>> {
    specs(instance.identity.game)
        .iter()
        .map(|spec| {
            let contents = read_document(paths, instance, *spec)?;
            let mut entries = parse(spec.format, &contents)?;
            entries.retain(|entry| !spec.managed.contains(&entry.key.as_str()));
            Ok(AdvancedConfigFile {
                id: spec.id.into(),
                path: spec.path.into(),
                format: format_name(spec.format),
                entries,
            })
        })
        .collect()
}

pub fn apply(
    paths: &Paths,
    instance: &GenericGameInstance,
    changes: &[AdvancedConfigChange],
) -> Result<()> {
    if instance.is_running() {
        bail!("stop the server before changing advanced configuration");
    }
    let mut per_file: HashMap<&str, Vec<&AdvancedConfigChange>> = HashMap::new();
    for change in changes {
        per_file
            .entry(change.file.as_str())
            .or_default()
            .push(change);
    }
    let mut writes = Vec::new();
    for (id, changes) in per_file {
        let spec = specs(instance.identity.game)
            .iter()
            .find(|spec| spec.id == id)
            .context("configuration file is not declared for this game")?;
        let original = read_document(paths, instance, *spec)?;
        let existing = parse(spec.format, &original)?;
        let keys: BTreeMap<_, _> = existing
            .iter()
            .map(|entry| (entry.key.as_str(), entry))
            .collect();
        let mut updated = original;
        for change in changes {
            if spec.managed.contains(&change.key.as_str()) {
                bail!("{} is managed by Odin", change.key);
            }
            if !keys.contains_key(change.key.as_str()) {
                bail!("{} does not exist in {}", change.key, spec.id);
            }
            updated = set(spec.format, &updated, &change.key, &change.value)?;
        }
        writes.push((document_path(paths, instance, *spec), updated));
    }
    // Write every temporary first.  Once that succeeds, rename each into
    // place; retain the old contents so a later rename error can be rolled
    // back instead of leaving a partially applied batch.
    let mut temps = Vec::new();
    for (path, contents) in &writes {
        let parent = path.parent().context("configuration path has no parent")?;
        fs::create_dir_all(parent)?;
        let temp = path.with_extension(format!("odin-{}.tmp", uuid::Uuid::new_v4()));
        fs::write(&temp, contents)?;
        temps.push((path.clone(), temp));
    }
    let mut previous = Vec::new();
    for ((path, temp), (_, contents)) in temps.iter().zip(writes.iter()) {
        let old = if path.is_file() {
            Some(fs::read(path)?)
        } else {
            None
        };
        if let Err(error) = fs::rename(temp, path) {
            for (rollback_path, rollback) in previous.into_iter().rev() {
                match rollback {
                    Some(bytes) => {
                        let _ = fs::write(rollback_path, bytes);
                    }
                    None => {
                        let _ = fs::remove_file(rollback_path);
                    }
                }
            }
            for (_, pending) in &temps {
                let _ = fs::remove_file(pending);
            }
            return Err(error.into());
        }
        let _ = contents;
        previous.push((path.clone(), old));
    }
    Ok(())
}

fn format_name(format: Format) -> &'static str {
    match format {
        Format::XmlProperties => "xml-properties",
        Format::Json => "json",
        Format::Ini => "ini",
        Format::PalworldOptions => "unreal-ini",
    }
}

fn parse(format: Format, contents: &str) -> Result<Vec<AdvancedConfigEntry>> {
    match format {
        Format::XmlProperties => {
            let pattern = regex::Regex::new(
                r#"(?is)<property\s+name\s*=\s*\"([^\"]+)\"[^>]*?value\s*=\s*\"([^\"]*)\"[^>]*/?>"#,
            )?;
            Ok(pattern
                .captures_iter(contents)
                .map(|capture| AdvancedConfigEntry {
                    key: capture[1].into(),
                    value: html_unescape(&capture[2]),
                })
                .collect())
        }
        Format::Json => {
            let value: Value =
                serde_json::from_str(contents).context("invalid JSON configuration")?;
            let mut entries = Vec::new();
            flatten_json("", &value, &mut entries);
            Ok(entries)
        }
        Format::Ini => Ok(parse_ini(contents)),
        Format::PalworldOptions => Ok(parse_palworld(contents)),
    }
}

fn flatten_json(prefix: &str, value: &Value, entries: &mut Vec<AdvancedConfigEntry>) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_json(&path, value, entries);
            }
        }
        Value::Array(_) => entries.push(AdvancedConfigEntry {
            key: prefix.into(),
            value: value.to_string(),
        }),
        Value::String(value) => entries.push(AdvancedConfigEntry {
            key: prefix.into(),
            value: value.clone(),
        }),
        _ => entries.push(AdvancedConfigEntry {
            key: prefix.into(),
            value: value.to_string(),
        }),
    }
}

fn parse_ini(contents: &str) -> Vec<AdvancedConfigEntry> {
    let mut section = String::new();
    let mut entries = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line.into();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            entries.push(AdvancedConfigEntry {
                key: if section.is_empty() {
                    key.trim().into()
                } else {
                    format!("{section}.{}", key.trim())
                },
                value: value.trim().into(),
            });
        }
    }
    entries
}

fn parse_palworld(contents: &str) -> Vec<AdvancedConfigEntry> {
    let Some(start) = contents.find("OptionSettings=(") else {
        return Vec::new();
    };
    let tail = &contents[start + "OptionSettings=(".len()..];
    let Some(end) = tail.find(')') else {
        return Vec::new();
    };
    tail[..end]
        .split(',')
        .filter_map(|part| {
            part.split_once('=')
                .map(|(key, value)| AdvancedConfigEntry {
                    key: key.trim().into(),
                    value: value.trim().trim_matches('"').into(),
                })
        })
        .collect()
}

fn set(format: Format, contents: &str, key: &str, value: &str) -> Result<String> {
    match format {
        Format::XmlProperties => set_xml(contents, key, value),
        Format::Json => set_json(contents, key, value),
        Format::Ini => set_ini(contents, key, value),
        Format::PalworldOptions => set_palworld(contents, key, value),
    }
}

fn set_xml(contents: &str, key: &str, value: &str) -> Result<String> {
    let expression = format!(
        r#"(?is)(<property\s+name\s*=\s*\"{}\"[^>]*?value\s*=\s*\")[^\"]*(\"[^>]*/?>)"#,
        regex::escape(key)
    );
    let pattern = regex::Regex::new(&expression)?;
    if !pattern.is_match(contents) {
        bail!("XML property {key} does not exist");
    }
    Ok(pattern
        .replace(contents, format!("${{1}}{}${{2}}", xml_escape(value)))
        .into_owned())
}

fn set_json(contents: &str, key: &str, value: &str) -> Result<String> {
    let mut document: Value = serde_json::from_str(contents)?;
    let mut target = &mut document;
    let mut parts = key.split('.').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            let object = target
                .as_object_mut()
                .context("JSON key is not an object property")?;
            let old = object.get(part).context("JSON key does not exist")?;
            object.insert(part.into(), parse_scalar(value, old));
        } else {
            target = target.get_mut(part).context("JSON key does not exist")?;
        }
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&document)?))
}

fn parse_scalar(value: &str, old: &Value) -> Value {
    match old {
        Value::Bool(_) => Value::Bool(matches!(value, "true" | "True" | "1")),
        Value::Number(number) if number.is_i64() => value
            .parse::<i64>()
            .map(Into::into)
            .unwrap_or_else(|_| Value::String(value.into())),
        Value::Number(_) => value
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.into())),
        Value::Null => Value::Null,
        _ => Value::String(value.into()),
    }
}

fn set_ini(contents: &str, key: &str, value: &str) -> Result<String> {
    let (_, bare) = key.rsplit_once('.').unwrap_or(("", key));
    let expression = format!(r"(?m)^(\s*{}\s*=\s*).*$", regex::escape(bare));
    let pattern = regex::Regex::new(&expression)?;
    if !pattern.is_match(contents) {
        bail!("INI key {key} does not exist");
    }
    Ok(pattern
        .replace(contents, format!("${{1}}{value}"))
        .into_owned())
}

fn set_palworld(contents: &str, key: &str, value: &str) -> Result<String> {
    let expression = format!(r#"({}=)(\"[^\"]*\"|[^,\)]*)"#, regex::escape(key));
    let pattern = regex::Regex::new(&expression)?;
    if !pattern.is_match(contents) {
        bail!("Palworld option {key} does not exist");
    }
    let quoted = format!("\"{}\"", value.replace('"', "\\\""));
    Ok(pattern
        .replace(contents, format!("${{1}}{quoted}"))
        .into_owned())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn html_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xml_round_trip_preserves_unknown_properties() {
        let input = "<ServerSettings><property name=\"Known\" value=\"old\"/><property name=\"Other\" value=\"yes\"/></ServerSettings>";
        assert_eq!(
            set_xml(input, "Known", "new").unwrap(),
            "<ServerSettings><property name=\"Known\" value=\"new\"/><property name=\"Other\" value=\"yes\"/></ServerSettings>"
        );
    }
    #[test]
    fn json_round_trip_preserves_unknown_properties() {
        let output = set_json(r#"{"a": {"b": 1}, "other": true}"#, "a.b", "2").unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["a"]["b"], 2);
        assert_eq!(parsed["other"], true);
    }
}
