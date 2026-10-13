//! Per-game configuration documents exposed by the dashboard.
//!
//! Odin owns only lifecycle-critical transport settings. Everything else stays
//! in the document created by the game and is exposed verbatim to the dashboard.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::db::game_instances::{GenericGameInstance, RustInstance};
use crate::game::{GameId, SEVEN_DAYS_TEMPLATE_BASELINE_FILE};
use crate::paths::Paths;

#[derive(Debug, Clone, Serialize)]
pub struct AdvancedConfigFile {
    pub id: String,
    pub path: String,
    pub format: &'static str,
    pub exists: bool,
    /// Whether operators may add previously absent keys to this document.
    pub allows_new_keys: bool,
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
    #[serde(default)]
    pub delete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawAdvancedConfigFile {
    pub content: String,
    pub format: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevenDaysTemplateReview {
    pub instance_config_exists: bool,
    pub template_exists: bool,
    pub baseline_exists: bool,
    pub changes: Vec<SevenDaysTemplateChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevenDaysTemplateChange {
    pub key: String,
    pub kind: SevenDaysTemplateChangeKind,
    pub applyable: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SevenDaysTemplateChangeKind {
    Added,
    Removed,
    DefaultChanged,
    Conflict,
}

#[derive(Clone, Copy)]
enum Format {
    XmlProperties,
    PalworldOptions,
    RustCfg,
    Ini,
}

#[derive(Clone, Copy)]
struct FileSpec {
    id: &'static str,
    path: &'static str,
    format: Format,
    managed: &'static [&'static str],
    allows_new_keys: bool,
}

const SEVEN_DAYS_MANAGED: &[&str] = &["ServerPort", "UserDataFolder", "TelnetPort"];
const PALWORLD_MANAGED: &[&str] = &[];
const RUST_MANAGED: &[&str] = &[
    "server.port",
    "server.queryport",
    "rcon.port",
    "rcon.password",
    "rcon.web",
    "server.identity",
];
const ARK_GAME_USER_SETTINGS_MANAGED: &[&str] = &[
    "[SessionSettings].SessionName",
    "[ServerSettings].RCONEnabled",
    "[ServerSettings].RCONPort",
    "[ServerSettings].ServerAdminPassword",
];

fn specs(game: GameId) -> &'static [FileSpec] {
    match game {
        GameId::SevenDaysToDie => &[FileSpec {
            id: "server",
            path: "config/serverconfig.xml",
            format: Format::XmlProperties,
            managed: SEVEN_DAYS_MANAGED,
            allows_new_keys: false,
        }],
        GameId::Palworld => &[FileSpec {
            id: "server",
            path: crate::game::palworld::CONFIG_FILE,
            format: Format::PalworldOptions,
            managed: PALWORLD_MANAGED,
            allows_new_keys: false,
        }],
        GameId::ArkSurvivalAscended => &[
            FileSpec {
                id: "game-user-settings",
                path: "Saved/Config/WindowsServer/GameUserSettings.ini",
                format: Format::Ini,
                managed: ARK_GAME_USER_SETTINGS_MANAGED,
                allows_new_keys: true,
            },
            FileSpec {
                id: "game",
                path: "Saved/Config/WindowsServer/Game.ini",
                format: Format::Ini,
                managed: &[],
                allows_new_keys: true,
            },
        ],
        GameId::Valheim | GameId::Rust => &[],
    }
}

fn rust_document_path(paths: &Paths, instance: &RustInstance) -> PathBuf {
    crate::game::rust::identity_dir(paths, instance).join("cfg/server.cfg")
}

pub fn initialize_rust(
    paths: &Paths,
    instance: &RustInstance,
    config: &crate::game::rust::RustFileConfig,
) -> Result<()> {
    let path = rust_document_path(paths, instance);
    anyhow::ensure!(!path.exists(), "{} already exists", path.display());
    create_atomically(&path, &rust_file_config(config))
}

pub fn migrate_rust_file_config(
    path: &Path,
    config: &crate::game::rust::RustFileConfig,
) -> Result<()> {
    let original = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let existing: BTreeSet<_> = parse_rust_cfg(&original)
        .into_iter()
        .map(|entry| entry.key)
        .collect();
    let all = rust_file_config_values(config);
    let missing: Vec<_> = all
        .into_iter()
        .filter(|(key, _)| !existing.contains(*key))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut updated = original;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    for (key, value) in missing {
        updated.push_str(key);
        updated.push(' ');
        updated.push_str(&value);
        updated.push('\n');
    }
    replace_or_create_atomically(path, &updated)
}

fn rust_file_config(config: &crate::game::rust::RustFileConfig) -> String {
    rust_file_config_values(config)
        .into_iter()
        .map(|(key, value)| format!("{key} {value}\n"))
        .collect()
}

fn rust_file_config_values(
    config: &crate::game::rust::RustFileConfig,
) -> [(&'static str, String); 5] {
    [
        ("server.hostname", rust_quote(&config.hostname)),
        ("server.level", rust_quote(&config.level)),
        ("server.seed", config.seed.to_string()),
        ("server.worldsize", config.world_size.to_string()),
        ("server.maxplayers", config.max_players.to_string()),
    ]
}

fn rust_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn list_rust(paths: &Paths, instance: &RustInstance) -> Result<Vec<AdvancedConfigFile>> {
    let path = rust_document_path(paths, instance);
    let contents = if path.is_file() {
        Some(fs::read_to_string(&path)?)
    } else {
        None
    };
    let sections = contents
        .as_deref()
        .map(|text| sections(Format::RustCfg, text, RUST_MANAGED))
        .transpose()?;
    Ok(vec![AdvancedConfigFile {
        id: "server".into(),
        path: "server/cfg/server.cfg".into(),
        format: "rust-cfg",
        exists: contents.is_some(),
        allows_new_keys: true,
        sections: sections.unwrap_or_default(),
    }])
}

pub fn apply_rust(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &RustInstance,
    changes: &[AdvancedConfigChange],
) -> Result<()> {
    if crate::game::rust::is_running(paths, db, instance)? {
        bail!("stop the server before changing advanced configuration");
    }
    let path = rust_document_path(paths, instance);
    let original = fs::read_to_string(&path)
        .with_context(|| format!("{} has not been created", path.display()))?;
    let mut keys = parse(Format::RustCfg, &original)?
        .into_iter()
        .map(|entry| entry.key)
        .collect::<std::collections::HashSet<_>>();
    let mut updated = original;
    for change in changes {
        if change.file != "server" {
            bail!("configuration file is not declared for Rust");
        }
        if RUST_MANAGED.contains(&change.key.as_str()) {
            bail!("{} is managed by Odin", change.key);
        }
        validate_rust_convar_name(&change.key)?;
        if change.delete {
            if !keys.remove(change.key.as_str()) {
                bail!("{} does not exist in server.cfg", change.key);
            }
            updated = remove_rust_cfg(&updated, &change.key)?;
            continue;
        }
        validate_rust_convar_value(&change.value)?;
        if !keys.contains(change.key.as_str()) {
            updated = append_rust_cfg(&updated, &change.key, &change.value);
            keys.insert(change.key.clone());
            continue;
        }
        if is_sensitive_key(&change.key) && change.value.is_empty() {
            continue;
        }
        updated = set(Format::RustCfg, &updated, &change.key, &change.value)?;
    }
    write_atomically(&path, &updated)
}

pub fn clone_rust(paths: &Paths, source: &RustInstance, target: &RustInstance) -> Result<()> {
    let source_path = rust_document_path(paths, source);
    let target_path = rust_document_path(paths, target);
    let mut contents = fs::read_to_string(&source_path)
        .with_context(|| format!("failed to read {}", source_path.display()))?;
    if parse_rust_cfg(&contents)
        .iter()
        .any(|entry| entry.key == "server.hostname")
    {
        contents = set_rust_cfg(&contents, "server.hostname", &rust_quote(target.name()))?;
    }
    replace_or_create_atomically(&target_path, &contents)
}

/// Keeps Odin-owned transport convars authoritative without inventing a
/// server.cfg or adding keys an operator did not put there.
pub fn sync_rust_operational(paths: &Paths, instance: &RustInstance) -> Result<()> {
    let path = rust_document_path(paths, instance);
    let mut contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let values = [
        ("server.port", instance.config.port.to_string()),
        ("server.queryport", instance.config.query_port.to_string()),
        ("rcon.port", instance.config.rcon_port.to_string()),
        ("rcon.password", instance.config.rcon_password.clone()),
        ("rcon.web", "1".into()),
        ("server.identity", instance.identity.id.clone()),
    ];
    let existing = parse(Format::RustCfg, &contents)?
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect::<HashMap<_, _>>();
    let mut changed = false;
    for (key, value) in values {
        if existing.get(key).is_some_and(|current| current != &value) {
            contents = set_rust_cfg(&contents, key, &value)?;
            changed = true;
        }
    }
    if changed {
        write_atomically(&path, &contents)?;
    }
    Ok(())
}

fn instance_root(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
    paths.game_instance_dir(instance.identity.game, instance.name())
}

fn shared_root(paths: &Paths, game: GameId) -> PathBuf {
    paths.shared_install_dir(game).join("ShooterGame")
}

fn document_path(root: &Path, spec: FileSpec) -> PathBuf {
    root.join(spec.path)
}

fn read_document(root: &Path, spec: FileSpec) -> Result<Option<String>> {
    let path = document_path(root, spec);
    if path.is_file() {
        fs::read_to_string(&path)
            .map(Some)
            .with_context(|| format!("failed to read {}", path.display()))
    } else {
        Ok(None)
    }
}

pub fn list(paths: &Paths, instance: &GenericGameInstance) -> Result<Vec<AdvancedConfigFile>> {
    list_at(&instance_root(paths, instance), instance.identity.game)
}

/// Shared ARK installs own one native configuration directory. This is kept
/// separate from the per-instance state directory so every shared instance
/// sees precisely the same game-owned settings.
pub fn list_shared(paths: &Paths, game: GameId) -> Result<Vec<AdvancedConfigFile>> {
    anyhow::ensure!(
        game == GameId::ArkSurvivalAscended,
        "this game has no shared configuration"
    );
    list_at(&shared_root(paths, game), game)
}

fn list_at(root: &Path, game: GameId) -> Result<Vec<AdvancedConfigFile>> {
    specs(game)
        .iter()
        .map(|spec| {
            let contents = read_document(root, *spec)?;
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
                allows_new_keys: spec.allows_new_keys,
                sections: sections.unwrap_or_default(),
            })
        })
        .collect()
}

fn seven_days_template_path(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
    crate::game::seven_days_to_die::template_path(paths, &instance.identity)
}

fn seven_days_baseline_path(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
    instance_root(paths, instance)
        .join("config")
        .join(SEVEN_DAYS_TEMPLATE_BASELINE_FILE)
}

fn seven_days_config_path(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
    document_path(
        &instance_root(paths, instance),
        specs(GameId::SevenDaysToDie)[0],
    )
}

/// Creates only missing native configuration documents. Existing operator
/// files are never replaced.
pub fn initialize(paths: &Paths, instance: &GenericGameInstance) -> Result<()> {
    match instance.identity.game {
        GameId::SevenDaysToDie => crate::game::seven_days_to_die::seed_config(
            &seven_days_template_path(paths, instance),
            &instance_root(paths, instance),
        ),
        GameId::Palworld => copy_template_if_missing(
            &crate::game::palworld::template_path(paths, &instance.identity),
            &document_path(&instance_root(paths, instance), specs(GameId::Palworld)[0]),
        ),
        GameId::ArkSurvivalAscended => {
            for spec in specs(GameId::ArkSurvivalAscended) {
                let path = document_path(&instance_root(paths, instance), *spec);
                if !path.exists() {
                    create_atomically(&path, "")?;
                }
            }
            Ok(())
        }
        GameId::Valheim | GameId::Rust => bail!("this game has no native configuration template"),
    }
}

pub fn initialize_shared(paths: &Paths, game: GameId) -> Result<()> {
    anyhow::ensure!(
        game == GameId::ArkSurvivalAscended,
        "this game has no shared configuration"
    );
    let root = shared_root(paths, game);
    for spec in specs(game) {
        let path = document_path(&root, *spec);
        if !path.exists() {
            create_atomically(&path, "")?;
        }
    }
    Ok(())
}

pub fn read_raw(
    paths: &Paths,
    instance: &GenericGameInstance,
    file: &str,
) -> Result<RawAdvancedConfigFile> {
    read_raw_at(
        &instance_root(paths, instance),
        instance.identity.game,
        file,
    )
}

pub fn read_raw_shared(paths: &Paths, game: GameId, file: &str) -> Result<RawAdvancedConfigFile> {
    anyhow::ensure!(
        game == GameId::ArkSurvivalAscended,
        "this game has no shared configuration"
    );
    read_raw_at(&shared_root(paths, game), game, file)
}

pub fn write_raw(
    paths: &Paths,
    instance: &GenericGameInstance,
    file: &str,
    content: &str,
) -> Result<()> {
    write_raw_at(
        &instance_root(paths, instance),
        instance.identity.game,
        file,
        content,
    )
}

pub fn write_raw_shared(paths: &Paths, game: GameId, file: &str, content: &str) -> Result<()> {
    anyhow::ensure!(
        game == GameId::ArkSurvivalAscended,
        "this game has no shared configuration"
    );
    write_raw_at(&shared_root(paths, game), game, file, content)
}

fn raw_spec(game: GameId, file: &str) -> Result<FileSpec> {
    specs(game)
        .iter()
        .copied()
        .find(|spec| spec.id == file)
        .context("configuration file is not declared for this game")
}

fn read_raw_at(root: &Path, game: GameId, file: &str) -> Result<RawAdvancedConfigFile> {
    let spec = raw_spec(game, file)?;
    let content = read_document(root, spec)?.unwrap_or_default();
    Ok(RawAdvancedConfigFile {
        content,
        format: format_name(spec.format),
    })
}

fn write_raw_at(root: &Path, game: GameId, file: &str, content: &str) -> Result<()> {
    const MAX_RAW_CONFIG_BYTES: usize = 1024 * 1024;
    anyhow::ensure!(
        content.len() <= MAX_RAW_CONFIG_BYTES,
        "configuration files may not exceed 1 MiB"
    );
    let spec = raw_spec(game, file)?;
    let updated = parse(spec.format, content)?;
    let path = document_path(root, spec);
    let current = read_document(root, spec)?.unwrap_or_default();
    let existing = parse(spec.format, &current)?;
    let values = |entries: Vec<ParsedEntry>| -> BTreeMap<String, String> {
        entries
            .into_iter()
            .map(|entry| (entry.key, entry.value))
            .collect()
    };
    let before = values(existing);
    let after = values(updated);
    for key in spec.managed {
        anyhow::ensure!(
            before.get(*key) == after.get(*key),
            "{key} is managed by Odin"
        );
    }
    replace_or_create_atomically(&path, content)
}

pub(crate) fn copy_template_if_missing(source: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        return Ok(());
    }
    anyhow::ensure!(
        source.is_file(),
        "configuration template not found at {}",
        source.display()
    );
    let contents = fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    create_atomically(target, &contents)
}

/// Compares the installed 7D2D template to the immutable template captured
/// when the instance configuration was created. The result never exposes
/// values, so passwords remain private while the dashboard can still show
/// which settings need an operator decision.
pub fn review_seven_days_template(
    paths: &Paths,
    instance: &GenericGameInstance,
) -> Result<SevenDaysTemplateReview> {
    anyhow::ensure!(
        instance.identity.game == GameId::SevenDaysToDie,
        "template review is only available for 7 Days to Die"
    );
    let config = seven_days_config_path(paths, instance);
    let template = seven_days_template_path(paths, instance);
    let baseline = seven_days_baseline_path(paths, instance);
    if !config.is_file() || !template.is_file() || !baseline.is_file() {
        return Ok(SevenDaysTemplateReview {
            instance_config_exists: config.is_file(),
            template_exists: template.is_file(),
            baseline_exists: baseline.is_file(),
            changes: Vec::new(),
        });
    }
    let current = xml_property_values(&fs::read_to_string(&config)?)?;
    let installed = xml_property_values(&fs::read_to_string(&template)?)?;
    let baseline = xml_property_values(&fs::read_to_string(&baseline)?)?;
    Ok(SevenDaysTemplateReview {
        instance_config_exists: true,
        template_exists: true,
        baseline_exists: true,
        changes: template_changes(&baseline, &installed, &current),
    })
}

pub fn apply_seven_days_template_review(
    paths: &Paths,
    instance: &GenericGameInstance,
    keys: &[String],
) -> Result<()> {
    let review = review_seven_days_template(paths, instance)?;
    anyhow::ensure!(
        review.instance_config_exists,
        "instance configuration does not exist"
    );
    anyhow::ensure!(
        review.template_exists,
        "installed 7 Days to Die configuration template does not exist"
    );
    anyhow::ensure!(
        review.baseline_exists,
        "instance has no configuration template baseline"
    );

    let config = seven_days_config_path(paths, instance);
    let template = seven_days_template_path(paths, instance);
    let selected: BTreeSet<_> = keys.iter().map(String::as_str).collect();
    let changes: BTreeMap<_, _> = review
        .changes
        .iter()
        .map(|change| (change.key.as_str(), change))
        .collect();
    for key in &selected {
        let change = changes
            .get(key)
            .context("selected configuration template change no longer exists")?;
        anyhow::ensure!(change.applyable, "{key} must be resolved manually");
    }

    let installed_contents = fs::read_to_string(&template)?;
    let installed = xml_property_values(&installed_contents)?;
    let mut contents = fs::read_to_string(&config)?;
    for key in selected {
        match changes[key].kind {
            SevenDaysTemplateChangeKind::Added => {
                contents = insert_xml_property(&contents, key, &installed[key])?;
            }
            SevenDaysTemplateChangeKind::Removed => {
                contents = remove_xml_property(&contents, key)?;
            }
            SevenDaysTemplateChangeKind::DefaultChanged => {
                contents = set_xml(&contents, key, &installed[key])?;
            }
            SevenDaysTemplateChangeKind::Conflict => unreachable!("conflicts are not applyable"),
        }
    }
    if !keys.is_empty() {
        write_atomically(&config, &contents)?;
    }
    fs::write(
        seven_days_baseline_path(paths, instance),
        installed_contents,
    )?;
    Ok(())
}

/// Starts tracking an existing configuration without guessing which values
/// came from an older game template and which the operator intentionally set.
pub fn adopt_seven_days_template_baseline(
    paths: &Paths,
    instance: &GenericGameInstance,
) -> Result<()> {
    let config = seven_days_config_path(paths, instance);
    let contents = fs::read_to_string(&config)
        .with_context(|| format!("failed to read {}", config.display()))?;
    xml_property_values(&contents)?;
    fs::write(seven_days_baseline_path(paths, instance), contents)
        .context("failed to save 7 Days to Die configuration template baseline")
}

fn xml_property_values(contents: &str) -> Result<BTreeMap<String, String>> {
    Ok(parse(Format::XmlProperties, contents)?
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect())
}

fn template_changes(
    baseline: &BTreeMap<String, String>,
    installed: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Vec<SevenDaysTemplateChange> {
    baseline
        .keys()
        .chain(installed.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|key| {
            match (baseline.get(key), installed.get(key), current.get(key)) {
                (None, Some(_), None) => Some((SevenDaysTemplateChangeKind::Added, true)),
                (None, Some(_), Some(_)) => Some((SevenDaysTemplateChangeKind::Conflict, false)),
                (Some(base), None, Some(current)) if current == base => {
                    Some((SevenDaysTemplateChangeKind::Removed, true))
                }
                (Some(_), None, _) => Some((SevenDaysTemplateChangeKind::Conflict, false)),
                (Some(base), Some(installed), Some(current))
                    if base != installed && current == base =>
                {
                    Some((SevenDaysTemplateChangeKind::DefaultChanged, true))
                }
                (Some(base), Some(installed), _) if base != installed => {
                    Some((SevenDaysTemplateChangeKind::Conflict, false))
                }
                _ => None,
            }
            .map(|(kind, applyable)| (key, kind, applyable))
        })
        .map(|(key, kind, applyable)| SevenDaysTemplateChange {
            key: (*key).clone(),
            kind,
            applyable,
        })
        .collect()
}

pub fn apply(
    paths: &Paths,
    instance: &GenericGameInstance,
    changes: &[AdvancedConfigChange],
) -> Result<()> {
    apply_at(
        &instance_root(paths, instance),
        instance.identity.game,
        changes,
    )
}

pub fn apply_shared(paths: &Paths, game: GameId, changes: &[AdvancedConfigChange]) -> Result<()> {
    anyhow::ensure!(
        game == GameId::ArkSurvivalAscended,
        "this game has no shared configuration"
    );
    apply_at(&shared_root(paths, game), game, changes)
}

fn apply_at(root: &Path, game: GameId, changes: &[AdvancedConfigChange]) -> Result<()> {
    let mut per_file: HashMap<&str, Vec<&AdvancedConfigChange>> = HashMap::new();
    for change in changes {
        per_file
            .entry(change.file.as_str())
            .or_default()
            .push(change);
    }
    let mut writes = Vec::new();
    for (id, changes) in per_file {
        let spec = specs(game)
            .iter()
            .find(|spec| spec.id == id)
            .context("configuration file is not declared for this game")?;
        let path = document_path(root, *spec);
        let original = read_document(root, *spec)?
            .with_context(|| format!("{} has not been generated by the game", path.display()))?;
        let existing = parse(spec.format, &original)?;
        let keys: BTreeMap<_, _> = existing
            .iter()
            .map(|entry| (entry.key.as_str(), entry))
            .collect();
        let mut updated = original;
        for change in changes {
            if change.delete && !spec.allows_new_keys {
                bail!("deleting advanced configuration keys is not supported for this game");
            }
            if spec.managed.contains(&change.key.as_str()) {
                bail!("{} is managed by Odin", change.key);
            }
            if !keys.contains_key(change.key.as_str()) && !spec.allows_new_keys {
                bail!("{} does not exist in {}", change.key, spec.id);
            }
            if change.delete {
                updated = remove(spec.format, &updated, &change.key)?;
                continue;
            }
            // A blank secret from the redacted dashboard means "keep the
            // current value", matching Odin's primary configuration forms.
            if is_sensitive_key(&change.key) && change.value.is_empty() {
                continue;
            }
            updated = if keys.contains_key(change.key.as_str()) {
                set(spec.format, &updated, &change.key, &change.value)?
            } else {
                append(spec.format, &updated, &change.key, &change.value)?
            };
        }
        writes.push((path, updated));
    }
    for (path, contents) in writes {
        write_atomically(&path, &contents)?;
    }
    Ok(())
}

/// Replaces an existing game-owned document without exposing a partially
/// written file. Its permissions remain unchanged so servers that run under a
/// dedicated account continue to own the same access contract.
fn write_atomically(path: &std::path::Path, contents: &str) -> Result<()> {
    let permissions = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?
        .permissions();
    let parent = path.parent().context("configuration path has no parent")?;
    let filename = path
        .file_name()
        .context("configuration path has no filename")?;
    let temporary = parent.join(format!(
        ".{}.odin-{}.tmp",
        filename.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        fs::write(&temporary, contents)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        fs::set_permissions(&temporary, permissions)
            .with_context(|| format!("failed to set permissions on {}", temporary.display()))?;
        fs::rename(&temporary, path).with_context(|| {
            format!(
                "failed to replace {} with {}",
                path.display(),
                temporary.display()
            )
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn create_atomically(path: &Path, contents: &str) -> Result<()> {
    anyhow::ensure!(!path.exists(), "{} already exists", path.display());
    replace_or_create_atomically(path, contents)
}

fn replace_or_create_atomically(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().context("configuration path has no parent")?;
    fs::create_dir_all(parent)?;
    if path.exists() {
        return write_atomically(path, contents);
    }
    let filename = path
        .file_name()
        .context("configuration path has no filename")?;
    let temporary = parent.join(format!(
        ".{}.odin-{}.tmp",
        filename.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        fs::write(&temporary, contents)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn format_name(format: Format) -> &'static str {
    match format {
        Format::XmlProperties => "xml-properties",
        Format::PalworldOptions => "unreal-ini",
        Format::RustCfg => "rust-cfg",
        Format::Ini => "ini",
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
        Format::PalworldOptions => parse_palworld(contents),
        Format::RustCfg => Ok(parse_rust_cfg(contents)),
        Format::Ini => Ok(parse_ini(contents)),
    }
}

fn parse_ini(contents: &str) -> Vec<ParsedEntry> {
    let mut section = "General";
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                return None;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len() - 1];
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            Some(ParsedEntry {
                key: format!("[{section}].{key}"),
                label: key.into(),
                section_id: section.into(),
                section_label: section.into(),
                value: value.trim().into(),
            })
        })
        .collect()
}

fn parse_rust_cfg(contents: &str) -> Vec<ParsedEntry> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once(char::is_whitespace)?;
            Some(ParsedEntry {
                key: key.into(),
                label: key.into(),
                section_id: "server".into(),
                section_label: "server.cfg".into(),
                value: value.trim().into(),
            })
        })
        .collect()
}

fn parse_palworld(contents: &str) -> Result<Vec<ParsedEntry>> {
    let Some((ini_section, range)) = palworld_option_settings(contents)? else {
        return Ok(Vec::new());
    };
    let section_label = format!("{ini_section} / OptionSettings");
    Ok(split_option_pairs(&contents[range])
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
        .collect())
}

fn split_option_pairs(input: &str) -> Vec<&str> {
    option_pair_ranges(input)
        .into_iter()
        .map(|range| &input[range])
        .collect()
}

fn option_pair_ranges(input: &str) -> Vec<Range<usize>> {
    let mut quoted = false;
    let mut escaped = false;
    let mut nesting = 0_usize;
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
        if !quoted && character == '(' {
            nesting += 1;
            continue;
        }
        if !quoted && character == ')' {
            nesting = nesting.saturating_sub(1);
            continue;
        }
        if character == ',' && !quoted && nesting == 0 {
            pairs.push(start..index);
            start = index + 1;
        }
    }
    pairs.push(start..input.len());
    pairs
}

fn palworld_option_settings(contents: &str) -> Result<Option<(&str, Range<usize>)>> {
    let assignment = regex::Regex::new(r"(?m)^\s*OptionSettings\s*=\s*\(")?;
    let Some(found) = assignment.find(contents) else {
        return Ok(None);
    };
    let opening = found.end() - 1;
    let closing = matching_parenthesis(contents, opening)
        .context("Palworld OptionSettings has an unclosed parenthesis")?;
    let ini_section = contents[..found.start()]
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('[') && line.ends_with(']'))
        .unwrap_or("General");
    Ok(Some((ini_section, opening + 1..closing)))
}

fn matching_parenthesis(contents: &str, opening: usize) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    let mut nesting = 0_u32;
    for (offset, character) in contents[opening..].char_indices() {
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
        if quoted {
            continue;
        }
        match character {
            '(' => nesting += 1,
            ')' => {
                nesting = nesting.checked_sub(1)?;
                if nesting == 0 {
                    return Some(opening + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Reads one property from a 7 Days to Die `serverconfig.xml` at `path`, for
/// callers that only have the instance directory (the container runtime).
pub fn xml_value(path: &Path, key: &str) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(parse(Format::XmlProperties, &contents)?
        .into_iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.value))
}

/// Reads a game-generated configuration value without exposing it through the
/// dashboard. Runtime integrations use this so credentials do not have a
/// second, potentially stale, copy in Odin's database.
pub fn value(paths: &Paths, instance: &GenericGameInstance, key: &str) -> Result<Option<String>> {
    let Some(spec) = specs(instance.identity.game).first().copied() else {
        return Ok(None);
    };
    let Some(contents) = read_document(&instance_root(paths, instance), spec)? else {
        return Ok(None);
    };
    Ok(parse(spec.format, &contents)?
        .into_iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.value))
}

/// Reads one option from a Palworld `PalWorldSettings.ini` at `path`, for
/// callers that only have the instance directory (the container runtime).
pub fn palworld_value(path: &Path, key: &str) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(parse(Format::PalworldOptions, &contents)?
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
    let root = instance_root(paths, instance);
    let Some(mut contents) = read_document(&root, spec)? else {
        return Ok(());
    };
    let values: Vec<(&str, String)> = match instance.identity.game {
        GameId::Palworld => Vec::new(),
        GameId::SevenDaysToDie => vec![
            ("ServerPort", instance.config.port.to_string()),
            (
                "TelnetPort",
                instance.config.admin_port.unwrap_or_default().to_string(),
            ),
        ],
        GameId::ArkSurvivalAscended | GameId::Valheim | GameId::Rust => Vec::new(),
    };
    let existing = parse(spec.format, &contents)?
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect::<HashMap<_, _>>();
    let mut changed = false;
    for (key, value) in values {
        if existing.get(key).is_some_and(|current| current != &value) {
            contents = set(spec.format, &contents, key, &value)?;
            changed = true;
        }
    }
    if changed {
        write_atomically(&document_path(&root, spec), &contents)?;
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
    let root = instance_root(paths, instance);
    let path = document_path(&root, spec);
    let Some(contents) = read_document(&root, spec)? else {
        return Ok(());
    };
    let pattern = regex::Regex::new(
        r#"(?is)\s*<property\b[^>]*\bname\s*=\s*(?:\"ControlPanelEnabled\"|'ControlPanelEnabled')[^>]*>"#,
    )?;
    let updated = pattern.replace_all(&contents, "");
    if updated != contents {
        write_atomically(&path, updated.as_ref())
            .with_context(|| format!("failed to repair {}", path.display()))?;
    }
    Ok(())
}

fn set(format: Format, contents: &str, key: &str, value: &str) -> Result<String> {
    match format {
        Format::XmlProperties => set_xml(contents, key, value),
        Format::PalworldOptions => set_palworld(contents, key, value),
        Format::RustCfg => set_rust_cfg(contents, key, value),
        Format::Ini => set_ini(contents, key, value),
    }
}

fn ini_key(key: &str) -> Result<(&str, &str)> {
    let (section, key) = key
        .strip_prefix('[')
        .and_then(|key| key.split_once("]."))
        .context("INI keys must use [Section].Key")?;
    anyhow::ensure!(
        !section.is_empty() && !key.is_empty(),
        "INI keys must use [Section].Key"
    );
    Ok((section, key))
}

fn set_ini(contents: &str, full_key: &str, value: &str) -> Result<String> {
    let (target_section, target_key) = ini_key(full_key)?;
    let mut section = "General";
    let mut found = false;
    let mut lines = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = &trimmed[1..trimmed.len() - 1];
        }
        if section == target_section
            && let Some((key, _)) = trimmed.split_once('=')
            && key.trim() == target_key
            && !found
        {
            lines.push(format!("{target_key}={value}"));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    anyhow::ensure!(found, "INI key {full_key} does not exist");
    Ok(format!("{}\n", lines.join("\n")))
}

fn append(format: Format, contents: &str, key: &str, value: &str) -> Result<String> {
    match format {
        Format::Ini => {
            let (section, key) = ini_key(key)?;
            let separator = if contents.is_empty() || contents.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            Ok(format!("{contents}{separator}[{section}]\n{key}={value}\n"))
        }
        _ => bail!("adding keys is not supported for this configuration format"),
    }
}

fn remove(format: Format, contents: &str, full_key: &str) -> Result<String> {
    match format {
        Format::Ini => {
            let (target_section, target_key) = ini_key(full_key)?;
            let mut section = "General";
            let mut found = false;
            let lines = contents
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        section = &trimmed[1..trimmed.len() - 1];
                    }
                    let remove = section == target_section
                        && trimmed
                            .split_once('=')
                            .is_some_and(|(key, _)| key.trim() == target_key);
                    found |= remove;
                    !remove
                })
                .collect::<Vec<_>>();
            anyhow::ensure!(found, "INI key {full_key} does not exist");
            Ok(if lines.is_empty() {
                String::new()
            } else {
                format!("{}\n", lines.join("\n"))
            })
        }
        _ => bail!("deleting keys is not supported for this configuration format"),
    }
}

fn set_rust_cfg(contents: &str, key: &str, value: &str) -> Result<String> {
    let replacement = format!("{key} {value}");
    let mut found = false;
    let output = contents
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if !found
                && !trimmed.starts_with('#')
                && !trimmed.starts_with("//")
                && trimmed.split_whitespace().next() == Some(key)
            {
                found = true;
                replacement.clone()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !found {
        bail!("Rust convar {key} does not exist");
    }
    Ok(if contents.ends_with('\n') {
        format!("{output}\n")
    } else {
        output
    })
}

fn append_rust_cfg(contents: &str, key: &str, value: &str) -> String {
    let separator = if contents.is_empty() || contents.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("{contents}{separator}{key} {value}\n")
}

fn validate_rust_convar_name(key: &str) -> Result<()> {
    anyhow::ensure!(
        !key.is_empty()
            && key
                .chars()
                .all(|character| character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '_' | '-')),
        "Rust convar names may contain only letters, numbers, dots, underscores, and hyphens"
    );
    Ok(())
}

fn validate_rust_convar_value(value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.contains('\n') && !value.contains('\r'),
        "Rust convar values cannot contain line breaks"
    );
    Ok(())
}

fn remove_rust_cfg(contents: &str, key: &str) -> Result<String> {
    let mut found = false;
    let output = contents
        .lines()
        .filter(|line| {
            let remove = rust_cfg_line_key(line) == Some(key);
            if remove {
                found = true;
            }
            !remove
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !found {
        bail!("Rust convar {key} does not exist");
    }
    Ok(if output.is_empty() {
        output
    } else if contents.ends_with('\n') {
        format!("{output}\n")
    } else {
        output
    })
}

fn rust_cfg_line_key(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    (!trimmed.starts_with('#') && !trimmed.starts_with("//"))
        .then(|| trimmed.split_whitespace().next())
        .flatten()
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

fn insert_xml_property(contents: &str, key: &str, value: &str) -> Result<String> {
    let closing = contents
        .rfind("</ServerSettings>")
        .context("serverconfig.xml has no ServerSettings element")?;
    let property = format!(
        r#"    <property name="{key}" value="{}"/>\n"#,
        xml_escape(value)
    );
    let mut output = String::with_capacity(contents.len() + property.len());
    output.push_str(&contents[..closing]);
    output.push_str(&property);
    output.push_str(&contents[closing..]);
    Ok(output)
}

fn remove_xml_property(contents: &str, key: &str) -> Result<String> {
    let pattern = regex::Regex::new(r"(?is)<property\b[^>]*>")?;
    let ranges = pattern
        .find_iter(contents)
        .filter(|tag| xml_attribute(tag.as_str(), "name").as_deref() == Some(key))
        .map(|tag| tag.range())
        .collect::<Vec<_>>();
    let mut output = contents.to_owned();
    for range in ranges.into_iter().rev() {
        output.replace_range(range, "");
    }
    Ok(output)
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

fn set_palworld(contents: &str, key: &str, value: &str) -> Result<String> {
    let Some((_, settings)) = palworld_option_settings(contents)? else {
        bail!("Palworld configuration has no OptionSettings tuple");
    };
    let options = &contents[settings.clone()];
    let pair = option_pair_ranges(options)
        .into_iter()
        .find(|range| {
            options[range.clone()]
                .split_once('=')
                .is_some_and(|(existing, _)| existing.trim() == key)
        })
        .with_context(|| format!("Palworld option {key} does not exist"))?;
    let equal = options[pair.clone()]
        .find('=')
        .expect("option pair was checked for equals");
    let value_start = settings.start + pair.start + equal + 1;
    let value_end = settings.start + pair.end;
    let previous = contents[value_start..value_end].trim();
    let replacement = if previous.starts_with('"') && previous.ends_with('"') {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    };
    Ok(format!(
        "{}{}{}",
        &contents[..value_start],
        replacement,
        &contents[value_end..]
    ))
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

    fn seven_days_instance() -> GenericGameInstance {
        GenericGameInstance {
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
                auto_restart: false,
            },
        }
    }

    fn generic_instance(game: GameId) -> GenericGameInstance {
        GenericGameInstance {
            identity: crate::db::game_instances::GameInstanceIdentity {
                id: "id".into(),
                game,
                name: "server".into(),
                created_at: chrono::Utc::now(),
                tags: Vec::new(),
            },
            config: crate::db::game_instances::default_generic_config(game, "server"),
        }
    }

    fn rust_instance() -> RustInstance {
        RustInstance {
            identity: crate::db::game_instances::GameInstanceIdentity {
                id: "id".into(),
                game: GameId::Rust,
                name: "rusty".into(),
                created_at: chrono::Utc::now(),
                tags: Vec::new(),
            },
            config: crate::db::game_instances::RustInstanceConfig {
                port: 28015,
                query_port: 28016,
                rcon_port: 28017,
                rcon_password: "secret".into(),
                auto_restart: false,
            },
        }
    }

    #[test]
    fn template_review_distinguishes_safe_updates_from_conflicts() {
        let values = |entries: &[(&str, &str)]| {
            entries
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into()))
                .collect::<BTreeMap<String, String>>()
        };
        let changes = template_changes(
            &values(&[("Changed", "old"), ("Removed", "old")]),
            &values(&[("Added", "new"), ("Changed", "new")]),
            &values(&[("Changed", "old"), ("Removed", "custom")]),
        );

        assert_eq!(changes.len(), 3);
        assert!(changes.iter().any(|change| {
            change.key == "Added"
                && change.kind == SevenDaysTemplateChangeKind::Added
                && change.applyable
        }));
        assert!(changes.iter().any(|change| {
            change.key == "Changed"
                && change.kind == SevenDaysTemplateChangeKind::DefaultChanged
                && change.applyable
        }));
        assert!(changes.iter().any(|change| {
            change.key == "Removed"
                && change.kind == SevenDaysTemplateChangeKind::Conflict
                && !change.applyable
        }));
    }

    #[test]
    fn template_review_applies_selected_safe_changes_and_advances_the_baseline() {
        let directory =
            std::env::temp_dir().join(format!("odin-template-review-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: directory.clone(),
            config_dir: directory,
        };
        let instance = seven_days_instance();
        let config = seven_days_config_path(&paths, &instance);
        let template = seven_days_template_path(&paths, &instance);
        let baseline = seven_days_baseline_path(&paths, &instance);
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::create_dir_all(template.parent().unwrap()).unwrap();
        fs::write(
            &baseline,
            "<ServerSettings><property name=\"Changed\" value=\"old\"/><property name=\"Removed\" value=\"old\"/></ServerSettings>",
        )
        .unwrap();
        fs::write(
            &template,
            "<ServerSettings><property name=\"Added\" value=\"new\"/><property name=\"Changed\" value=\"new\"/></ServerSettings>",
        )
        .unwrap();
        fs::write(
            &config,
            "<ServerSettings><property name=\"Changed\" value=\"old\"/><property name=\"Removed\" value=\"custom\"/></ServerSettings>",
        )
        .unwrap();

        apply_seven_days_template_review(&paths, &instance, &["Added".into(), "Changed".into()])
            .unwrap();

        let updated = fs::read_to_string(&config).unwrap();
        assert!(updated.contains("name=\"Added\" value=\"new\""));
        assert!(updated.contains("name=\"Changed\" value=\"new\""));
        assert!(updated.contains("name=\"Removed\" value=\"custom\""));
        assert_eq!(
            fs::read_to_string(baseline).unwrap(),
            fs::read_to_string(template).unwrap()
        );
        fs::remove_dir_all(paths.data_dir).unwrap();
    }

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
    fn palworld_parser_keeps_commas_inside_quoted_values() {
        let entries =
            parse_palworld("OptionSettings=(Known=1,Message=\"one, two\",Other=True)").unwrap();
        assert_eq!(entries[1].key, "Message");
        assert_eq!(entries[1].value, "one, two");
    }

    #[test]
    fn palworld_options_handle_parentheses_and_only_update_the_option_tuple() {
        let input = "[/Script/Pal.PalGameWorldSettings]\nServerName=outside\nOptionSettings=(ServerName=\"The (best), server\",Nested=(One,Two),Other=True)\n";
        let entries = parse_palworld(input).unwrap();
        assert_eq!(entries[0].value, "The (best), server");
        assert_eq!(entries[1].key, "Nested");

        let output = set_palworld(input, "ServerName", "Updated").unwrap();
        assert!(output.contains("ServerName=outside"));
        assert!(
            output.contains("OptionSettings=(ServerName=\"Updated\",Nested=(One,Two),Other=True)")
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory =
            std::env::temp_dir().join(format!("odin-config-write-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("serverconfig.xml");
        fs::write(&file, "before").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();

        write_atomically(&file, "after").unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "after");
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o640
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn native_document_structure_becomes_dashboard_sections() {
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
    fn palworld_initializes_and_edits_advertised_and_rest_ports_in_the_ini() {
        let directory = std::env::temp_dir().join(format!(
            "odin-palworld-config-init-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = Paths {
            data_dir: directory.clone(),
            config_dir: directory,
        };
        let template = paths
            .game_install_dir(GameId::Palworld)
            .join("DefaultPalWorldSettings.ini");
        fs::create_dir_all(template.parent().unwrap()).unwrap();
        fs::write(
            &template,
            "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(PublicPort=8211,RESTAPIPort=8212,ServerName=Odin)\n",
        )
        .unwrap();
        let instance = generic_instance(GameId::Palworld);
        initialize(&paths, &instance).unwrap();
        apply(
            &paths,
            &instance,
            &[
                AdvancedConfigChange {
                    file: "server".into(),
                    key: "PublicPort".into(),
                    value: "9000".into(),
                    delete: false,
                },
                AdvancedConfigChange {
                    file: "server".into(),
                    key: "RESTAPIPort".into(),
                    value: "9001".into(),
                    delete: false,
                },
            ],
        )
        .unwrap();
        let active = document_path(
            &instance_root(&paths, &instance),
            specs(GameId::Palworld)[0],
        );
        let contents = fs::read_to_string(active).unwrap();
        assert!(contents.contains("PublicPort=9000"));
        assert!(contents.contains("RESTAPIPort=9001"));
        fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn rust_cfg_round_trip_preserves_comments_and_other_convars() {
        let input = "// keep this\nserver.hostname \"Old name\"\nserver.maxplayers 20\n";
        let entries = parse(Format::RustCfg, input).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "server.hostname");
        assert_eq!(
            set_rust_cfg(input, "server.hostname", "\"New name\"").unwrap(),
            "// keep this\nserver.hostname \"New name\"\nserver.maxplayers 20\n"
        );
    }

    #[test]
    fn rust_configuration_allows_operators_to_add_safe_convars() {
        let directory =
            std::env::temp_dir().join(format!("odin-rust-config-add-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: directory.clone(),
            config_dir: directory.clone(),
        };
        let instance = rust_instance();
        let path = rust_document_path(&paths, &instance);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "// keep this\nserver.hostname \"Odin\"\n").unwrap();
        let db = crate::db::Db::open(&paths).unwrap();

        assert!(list_rust(&paths, &instance).unwrap()[0].allows_new_keys);
        apply_rust(
            &paths,
            &db,
            &instance,
            &[AdvancedConfigChange {
                file: "server".into(),
                key: "server.description".into(),
                value: "\"A Rust server\"".into(),
                delete: false,
            }],
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "// keep this\nserver.hostname \"Odin\"\nserver.description \"A Rust server\"\n"
        );
        apply_rust(
            &paths,
            &db,
            &instance,
            &[AdvancedConfigChange {
                file: "server".into(),
                key: "server.description".into(),
                value: String::new(),
                delete: true,
            }],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "// keep this\nserver.hostname \"Odin\"\n"
        );
        assert!(
            apply_rust(
                &paths,
                &db,
                &instance,
                &[AdvancedConfigChange {
                    file: "server".into(),
                    key: "rcon.port".into(),
                    value: "28018".into(),
                    delete: false,
                }],
            )
            .is_err()
        );
        assert!(
            apply_rust(
                &paths,
                &db,
                &instance,
                &[AdvancedConfigChange {
                    file: "server".into(),
                    key: "server.motd".into(),
                    value: "hello\nserver.port 28016".into(),
                    delete: false,
                }],
            )
            .is_err()
        );
        fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn operational_sync_does_not_rewrite_matching_palworld_settings() {
        let dir = std::env::temp_dir().join(format!(
            "odin-palworld-config-sync-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let instance = GenericGameInstance {
            identity: crate::db::game_instances::GameInstanceIdentity {
                id: "id".into(),
                game: GameId::Palworld,
                name: "pals".into(),
                created_at: chrono::Utc::now(),
                tags: Vec::new(),
            },
            config: crate::db::game_instances::GenericGameConfig {
                port: 8211,
                query_port: Some(27015),
                admin_port: None,
                auto_restart: false,
            },
        };
        let path = document_path(
            &instance_root(&paths, &instance),
            specs(GameId::Palworld)[0],
        );
        let contents = "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(PublicPort=8211,RESTAPIPort=8212,ServerName=Odin)\n";
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();

        sync_operational(&paths, &instance).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[test]
    fn rust_operational_sync_does_not_rewrite_matching_convars() {
        let dir =
            std::env::temp_dir().join(format!("odin-rust-config-sync-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let instance = RustInstance {
            identity: crate::db::game_instances::GameInstanceIdentity {
                id: "id".into(),
                game: GameId::Rust,
                name: "rusty".into(),
                created_at: chrono::Utc::now(),
                tags: Vec::new(),
            },
            config: crate::db::game_instances::RustInstanceConfig {
                port: 28015,
                query_port: 28016,
                rcon_port: 28017,
                rcon_password: "secret".into(),
                auto_restart: false,
            },
        };
        let path = rust_document_path(&paths, &instance);
        let contents = "// preserve this comment\nserver.port 28015\nserver.queryport 28016\nrcon.port 28017\nrcon.password secret\nrcon.web 1\n";
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();

        sync_rust_operational(&paths, &instance).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        fs::remove_dir_all(paths.data_dir).unwrap();
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
                auto_restart: false,
            },
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
                    value: "Odin".into(),
                    delete: false,
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

    #[test]
    fn shared_ark_configuration_is_initialized_and_updated_outside_instance_state() {
        let dir =
            std::env::temp_dir().join(format!("odin-shared-ark-config-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };

        initialize_shared(&paths, GameId::ArkSurvivalAscended).unwrap();
        apply_shared(
            &paths,
            GameId::ArkSurvivalAscended,
            &[AdvancedConfigChange {
                file: "game".into(),
                key: "[/Script/ShooterGame.ShooterGameMode].XPMultiplier".into(),
                value: "2.0".into(),
                delete: false,
            }],
        )
        .unwrap();

        let config = paths
            .shared_install_dir(GameId::ArkSurvivalAscended)
            .join("ShooterGame/Saved/Config/WindowsServer/Game.ini");
        assert!(
            fs::read_to_string(config)
                .unwrap()
                .contains("XPMultiplier=2.0")
        );
        assert!(list_shared(&paths, GameId::ArkSurvivalAscended).unwrap()[1].exists);
        assert!(
            !paths
                .game_instance_dir(GameId::ArkSurvivalAscended, "server")
                .exists()
        );
        fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn raw_ark_editor_rejects_managed_key_changes() {
        let dir =
            std::env::temp_dir().join(format!("odin-raw-ark-config-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        initialize_shared(&paths, GameId::ArkSurvivalAscended).unwrap();
        let root = shared_root(&paths, GameId::ArkSurvivalAscended);
        let config = document_path(&root, specs(GameId::ArkSurvivalAscended)[0]);
        fs::write(
            &config,
            "[SessionSettings]\nSessionName=Odin\n[ServerSettings]\nRCONEnabled=True\n",
        )
        .unwrap();

        assert!(
            write_raw_shared(
                &paths,
                GameId::ArkSurvivalAscended,
                "game-user-settings",
                "[SessionSettings]\nSessionName=Other\n[ServerSettings]\nRCONEnabled=True\n",
            )
            .is_err()
        );
        write_raw_shared(
            &paths,
            GameId::ArkSurvivalAscended,
            "game-user-settings",
            "[SessionSettings]\nSessionName=Odin\n[ServerSettings]\nRCONEnabled=True\nDifficultyOffset=1.0\n",
        ).unwrap();
        assert!(
            fs::read_to_string(config)
                .unwrap()
                .contains("DifficultyOffset=1.0")
        );
        fs::remove_dir_all(paths.data_dir).ok();
    }
}
