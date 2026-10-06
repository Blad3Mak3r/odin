//! Per-game configuration documents exposed by the dashboard.
//!
//! Odin owns only lifecycle-critical transport settings. Everything else stays
//! in the document created by the game and is exposed verbatim to the dashboard.

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
    pub exists: bool,
    pub sections: Vec<AdvancedConfigSection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvancedConfigSection {
    /// Stable structural identifier, used only by the dashboard.
    pub id: String,
    /// Human-readable source section name.
    pub label: String,
    pub entries: Vec<AdvancedConfigEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvancedConfigEntry {
    /// Full key understood by the document writer.
    pub key: String,
    /// The local property name shown inside its section.
    pub label: String,
    pub value: String,
    pub sensitive: bool,
    pub configured: bool,
    pub managed: bool,
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

const SEVEN_DAYS_MANAGED: &[&str] = &["ServerPort", "UserDataFolder", "TelnetPort"];
const VRISING_MANAGED: &[&str] = &["Port", "QueryPort", "Rcon.Port", "Rcon.BindAddress"];
const PALWORLD_MANAGED: &[&str] = &["PublicPort", "RESTAPIPort"];
const DRAGONWILDS_MANAGED: &[&str] = &[];

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

fn read_document(
    paths: &Paths,
    instance: &GenericGameInstance,
    spec: FileSpec,
) -> Result<Option<String>> {
    let path = document_path(paths, instance, spec);
    if path.is_file() {
        fs::read_to_string(&path)
            .map(Some)
            .with_context(|| format!("failed to read {}", path.display()))
    } else {
        Ok(None)
    }
}

pub fn list(paths: &Paths, instance: &GenericGameInstance) -> Result<Vec<AdvancedConfigFile>> {
    specs(instance.identity.game)
        .iter()
        .map(|spec| {
            let contents = read_document(paths, instance, *spec)?;
            let exists = contents.is_some();
            let sections = contents
                .as_deref()
                .map(|contents| sections(spec.format, contents, spec.managed))
                .transpose()?;
            Ok(AdvancedConfigFile {
                id: spec.id.into(),
                path: spec.path.into(),
                format: format_name(spec.format),
                exists,
                sections: sections.unwrap_or_default(),
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
        let path = document_path(paths, instance, *spec);
        let original = read_document(paths, instance, *spec)?
            .with_context(|| format!("{} has not been generated by the game", path.display()))?;
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
            // A blank secret from the redacted dashboard means "keep the
            // current value", matching Odin's primary configuration forms.
            if is_sensitive_key(&change.key) && change.value.is_empty() {
                continue;
            }
            updated = set(spec.format, &updated, &change.key, &change.value)?;
        }
        writes.push((path, updated));
    }
    // Write every temporary first.  Once that succeeds, rename each into
    // place; retain the old contents so a later rename error can be rolled
    // back instead of leaving a partially applied batch.
    let mut temps = Vec::new();
    for (path, contents) in &writes {
        let _parent = path.parent().context("configuration path has no parent")?;
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

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("password")
        || key.contains("secret")
        || key.contains("token")
        || key.ends_with("key")
}

#[derive(Debug, Clone)]
struct ParsedEntry {
    key: String,
    label: String,
    section_id: String,
    section_label: String,
    value: String,
}

fn sections(
    format: Format,
    contents: &str,
    managed: &[&str],
) -> Result<Vec<AdvancedConfigSection>> {
    let mut sections = Vec::<AdvancedConfigSection>::new();
    for parsed in parse(format, contents)? {
        let sensitive = is_sensitive_key(&parsed.key);
        let configured = !parsed.value.is_empty();
        let entry = AdvancedConfigEntry {
            key: parsed.key.clone(),
            label: parsed.label,
            value: if sensitive {
                String::new()
            } else {
                parsed.value
            },
            sensitive,
            configured,
            managed: managed.contains(&parsed.key.as_str()),
        };
        if let Some(section) = sections
            .iter_mut()
            .find(|section| section.id == parsed.section_id)
        {
            section.entries.push(entry);
        } else {
            sections.push(AdvancedConfigSection {
                id: parsed.section_id,
                label: parsed.section_label,
                entries: vec![entry],
            });
        }
    }
    Ok(sections)
}

fn parse(format: Format, contents: &str) -> Result<Vec<ParsedEntry>> {
    match format {
        Format::XmlProperties => {
            let pattern = regex::Regex::new(r"(?is)<property\b[^>]*>")?;
            Ok(pattern
                .find_iter(contents)
                .filter_map(|tag| {
                    let tag = tag.as_str();
                    let key = xml_attribute(tag, "name")?;
                    Some(ParsedEntry {
                        label: key.clone(),
                        key,
                        section_id: "ServerSettings".into(),
                        section_label: "ServerSettings".into(),
                        value: html_unescape(&xml_attribute(tag, "value")?),
                    })
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

fn flatten_json(prefix: &str, value: &Value, entries: &mut Vec<ParsedEntry>) {
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
        Value::Array(_) => push_json_entry(entries, prefix, value.to_string()),
        Value::String(value) => push_json_entry(entries, prefix, value.clone()),
        _ => push_json_entry(entries, prefix, value.to_string()),
    }
}

fn push_json_entry(entries: &mut Vec<ParsedEntry>, key: &str, value: String) {
    let (section_id, label) = key.rsplit_once('.').unwrap_or(("root", key));
    entries.push(ParsedEntry {
        key: key.into(),
        label: label.into(),
        section_id: section_id.into(),
        section_label: if section_id == "root" {
            "General".into()
        } else {
            section_id.into()
        },
        value,
    });
}

fn parse_ini(contents: &str) -> Vec<ParsedEntry> {
    let mut section = String::new();
    let mut entries = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line.into();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let label = key.trim();
            entries.push(ParsedEntry {
                key: if section.is_empty() {
                    label.into()
                } else {
                    format!("{section}.{label}")
                },
                label: label.into(),
                section_id: if section.is_empty() {
                    "root".into()
                } else {
                    section.clone()
                },
                section_label: if section.is_empty() {
                    "General".into()
                } else {
                    section.clone()
                },
                value: value.trim().into(),
            });
        }
    }
    entries
}

fn parse_palworld(contents: &str) -> Vec<ParsedEntry> {
    let Some(start) = contents.find("OptionSettings=(") else {
        return Vec::new();
    };
    let tail = &contents[start + "OptionSettings=(".len()..];
    let Some(end) = tail.find(')') else {
        return Vec::new();
    };
    let ini_section = contents[..start]
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('[') && line.ends_with(']'))
        .unwrap_or("General");
    let section_label = format!("{ini_section} / OptionSettings");
    split_option_pairs(&tail[..end])
        .into_iter()
        .filter_map(|part| {
            part.split_once('=').map(|(key, value)| ParsedEntry {
                key: key.trim().into(),
                label: key.trim().into(),
                section_id: format!("{ini_section}.OptionSettings"),
                section_label: section_label.clone(),
                value: value.trim().trim_matches('"').into(),
            })
        })
        .collect()
}

fn split_option_pairs(input: &str) -> Vec<&str> {
    let mut quoted = false;
    let mut escaped = false;
    let mut start = 0;
    let mut pairs = Vec::new();
    for (index, character) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quoted {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
            continue;
        }
        if character == ',' && !quoted {
            pairs.push(&input[start..index]);
            start = index + 1;
        }
    }
    pairs.push(&input[start..]);
    pairs
}

/// Reads a game-generated configuration value without exposing it through the
/// dashboard. Runtime integrations use this so credentials do not have a
/// second, potentially stale, copy in Odin's database.
pub fn value(paths: &Paths, instance: &GenericGameInstance, key: &str) -> Result<Option<String>> {
    let Some(spec) = specs(instance.identity.game).first().copied() else {
        return Ok(None);
    };
    let Some(contents) = read_document(paths, instance, spec)? else {
        return Ok(None);
    };
    Ok(parse(spec.format, &contents)?
        .into_iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.value))
}

/// Mirrors Odin's transport ports only into already-existing properties. This
/// deliberately never creates a game configuration file or a missing key.
pub fn sync_operational(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    let Some(spec) = specs(instance.identity.game).first().copied() else {
        return Ok(());
    };
    let Some(mut contents) = read_document(paths, instance, spec)? else {
        return Ok(());
    };
    let values: Vec<(&str, String)> = match instance.identity.game {
        GameId::VRising => vec![
            ("Port", instance.config.port.to_string()),
            (
                "QueryPort",
                instance.config.query_port.unwrap_or_default().to_string(),
            ),
            (
                "Rcon.Port",
                instance.config.admin_port.unwrap_or_default().to_string(),
            ),
        ],
        GameId::Palworld => vec![
            ("PublicPort", instance.config.port.to_string()),
            (
                "RESTAPIPort",
                instance.config.admin_port.unwrap_or_default().to_string(),
            ),
        ],
        GameId::SevenDaysToDie => vec![
            ("ServerPort", instance.config.port.to_string()),
            (
                "TelnetPort",
                instance.config.admin_port.unwrap_or_default().to_string(),
            ),
        ],
        GameId::RunescapeDragonwilds | GameId::Valheim | GameId::Rust => Vec::new(),
    };
    let keys = parse(spec.format, &contents)?
        .into_iter()
        .map(|entry| entry.key)
        .collect::<std::collections::HashSet<_>>();
    let mut changed = false;
    for (key, value) in values {
        if keys.contains(key) {
            contents = set(spec.format, &contents, key, &value)?;
            changed = true;
        }
    }
    if changed {
        fs::write(document_path(paths, instance, spec), contents)?;
    }
    Ok(())
}

/// Removes the one obsolete option Odin used to add to 7DTD configurations.
/// It is intentionally idempotent and never creates a file.
pub fn remove_legacy_control_panel(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    if instance.identity.game != GameId::SevenDaysToDie {
        return Ok(());
    }
    let spec = specs(GameId::SevenDaysToDie)[0];
    let path = document_path(paths, instance, spec);
    let Some(contents) = read_document(paths, instance, spec)? else {
        return Ok(());
    };
    let pattern = regex::Regex::new(
        r#"(?is)\s*<property\b[^>]*\bname\s*=\s*(?:\"ControlPanelEnabled\"|'ControlPanelEnabled')[^>]*>"#,
    )?;
    let updated = pattern.replace_all(&contents, "");
    if updated != contents {
        fs::write(&path, updated.as_ref())
            .with_context(|| format!("failed to repair {}", path.display()))?;
    }
    Ok(())
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
    let pattern = regex::Regex::new(r"(?is)<property\b[^>]*>")?;
    for tag in pattern.find_iter(contents) {
        if xml_attribute(tag.as_str(), "name").as_deref() == Some(key) {
            let replacement = format!(r#"<property name="{key}" value="{}"/>"#, xml_escape(value));
            let mut output = String::with_capacity(contents.len() + replacement.len());
            output.push_str(&contents[..tag.start()]);
            output.push_str(&replacement);
            output.push_str(&contents[tag.end()..]);
            return Ok(output);
        }
    }
    bail!("XML property {key} does not exist")
}

fn xml_attribute(tag: &str, name: &str) -> Option<String> {
    let expression = format!(
        r#"(?is)\b{}\s*=\s*(?:"([^"]*)"|'([^']*)')"#,
        regex::escape(name)
    );
    let pattern = regex::Regex::new(&expression).ok()?;
    let captures = pattern.captures(tag)?;
    captures
        .get(1)
        .or_else(|| captures.get(2))
        .map(|value| value.as_str().into())
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
    let (wanted_section, bare) = key.rsplit_once('.').unwrap_or(("", key));
    let mut section = "";
    let mut found = false;
    let mut output = String::with_capacity(contents.len());
    for raw_line in contents.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let ending = &raw_line[line.len()..];
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed;
            output.push_str(raw_line);
            continue;
        }
        if section == wanted_section
            && let Some((existing, _)) = line.split_once('=')
            && existing.trim() == bare
        {
            let prefix = &line[..line.find('=').expect("split_once found equals") + 1];
            output.push_str(prefix);
            output.push_str(value);
            output.push_str(ending);
            found = true;
            continue;
        }
        output.push_str(raw_line);
    }
    if !found {
        bail!("INI key {key} does not exist");
    }
    Ok(output)
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
    fn xml_properties_accept_reordered_single_quoted_attributes() {
        let entries = parse(
            Format::XmlProperties,
            "<ServerSettings><property value='old' name='Known'/></ServerSettings>",
        )
        .unwrap();
        assert_eq!(entries[0].key, "Known");
        assert_eq!(
            set_xml("<property value='old' name='Known'/>", "Known", "new").unwrap(),
            "<property name=\"Known\" value=\"new\"/>"
        );
    }
    #[test]
    fn json_round_trip_preserves_unknown_properties() {
        let output = set_json(r#"{"a": {"b": 1}, "other": true}"#, "a.b", "2").unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["a"]["b"], 2);
        assert_eq!(parsed["other"], true);
    }

    #[test]
    fn ini_round_trip_preserves_other_sections() {
        let input = "[Server]\nKnown=old\n[Other]\nKnown=other\n";
        let output = set_ini(input, "[Server].Known", "new").unwrap();
        assert!(output.contains("Known=new"));
        assert!(output.contains("[Other]\nKnown=other"));
    }

    #[test]
    fn palworld_parser_keeps_commas_inside_quoted_values() {
        let entries = parse_palworld("OptionSettings=(Known=1,Message=\"one, two\",Other=True)");
        assert_eq!(entries[1].key, "Message");
        assert_eq!(entries[1].value, "one, two");
    }

    #[test]
    fn native_document_structure_becomes_dashboard_sections() {
        let ini = sections(Format::Ini, "[Server]\nName=Odin\n[Rules]\nPvP=true\n", &[]).unwrap();
        assert_eq!(ini.len(), 2);
        assert_eq!(ini[0].label, "[Server]");
        assert_eq!(ini[0].entries[0].label, "Name");
        assert_eq!(ini[1].entries[0].key, "[Rules].PvP");

        let json = sections(
            Format::Json,
            r#"{"Rcon":{"Enabled":true,"Port":25575}}"#,
            &[],
        )
        .unwrap();
        assert_eq!(json[0].label, "Rcon");
        assert_eq!(json[0].entries[0].key, "Rcon.Enabled");

        let xml = sections(
            Format::XmlProperties,
            "<ServerSettings><property name=\"ServerName\" value=\"Odin\"/></ServerSettings>",
            &[],
        )
        .unwrap();
        assert_eq!(xml[0].label, "ServerSettings");
        assert_eq!(xml[0].entries[0].label, "ServerName");
    }

    #[test]
    fn palworld_uses_its_ini_and_option_settings_as_one_section() {
        let sections = sections(
            Format::PalworldOptions,
            "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(ServerName=Odin)",
            &[],
        )
        .unwrap();
        assert_eq!(
            sections[0].label,
            "[/Script/Pal.PalGameWorldSettings] / OptionSettings"
        );
        assert_eq!(sections[0].entries[0].label, "ServerName");
    }

    #[test]
    fn missing_document_is_reported_without_creating_a_template() {
        let dir =
            std::env::temp_dir().join(format!("odin-config-document-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let instance = GenericGameInstance {
            identity: crate::db::game_instances::GameInstanceIdentity {
                id: "id".into(),
                game: GameId::SevenDaysToDie,
                name: "undead".into(),
                created_at: chrono::Utc::now(),
                tags: Vec::new(),
            },
            config: crate::db::game_instances::GenericGameConfig {
                port: 26900,
                query_port: None,
                admin_port: Some(26903),
                settings: Value::Object(Default::default()),
                auto_restart: false,
            },
            pid: None,
            pid_started_at: None,
            last_started_at: None,
            last_stopped_at: None,
        };
        let files = list(&paths, &instance).unwrap();
        assert!(!files[0].exists);
        assert!(files[0].sections.is_empty());
        assert!(apply(&paths, &instance, &[]).is_ok());
        assert!(
            apply(
                &paths,
                &instance,
                &[AdvancedConfigChange {
                    file: "server".into(),
                    key: "ServerName".into(),
                    value: "Odin".into()
                }]
            )
            .is_err()
        );
        let config = paths
            .game_instance_dir(GameId::SevenDaysToDie, "undead")
            .join("config/serverconfig.xml");
        assert!(!config.exists());
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            "<ServerSettings><property name=\"ControlPanelEnabled\" value=\"false\"/><property name=\"WebDashboardEnabled\" value=\"false\"/></ServerSettings>",
        )
        .unwrap();
        remove_legacy_control_panel(&paths, &instance).unwrap();
        let repaired = std::fs::read_to_string(config).unwrap();
        assert!(!repaired.contains("ControlPanelEnabled"));
        assert!(repaired.contains("WebDashboardEnabled"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
