//! Game-neutral identity records plus Rust's v1 configuration.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;

use crate::cli::validate_instance_name;
use crate::game::{GameId, SEVEN_DAYS_TEMPLATE_BASELINE_FILE, rust};
use crate::paths::Paths;

#[derive(Debug, Clone, Serialize)]
pub struct GameInstanceIdentity {
    pub tags: Vec<String>,
    pub id: String,
    pub game: GameId,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RustInstanceConfig {
    pub port: u16,
    pub query_port: u16,
    pub rcon_port: u16,
    pub rcon_password: String,
    pub hostname: String,
    pub level: String,
    pub seed: u32,
    pub world_size: u32,
    pub max_players: u16,
    pub auto_restart: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidRustConfig(pub String);

#[derive(Debug, Clone, Serialize)]
pub struct RustInstance {
    #[serde(flatten)]
    pub identity: GameInstanceIdentity,
    #[serde(flatten)]
    pub config: RustInstanceConfig,
    pub pid: Option<u32>,
    pub pid_started_at: Option<i64>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_stopped_at: Option<DateTime<Utc>>,
}

impl RustInstance {
    pub fn is_running(&self) -> bool {
        rust::is_running(self)
    }

    pub fn name(&self) -> &str {
        &self.identity.name
    }
}

/// Persisted state for a compiled driver whose settings are not part of
/// Odin's historical Valheim/Rust schemas. Keeping the typed transport fields
/// separate from `settings` gives the supervisor a stable lifecycle contract
/// while each driver owns its own configuration document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericGameConfig {
    pub port: u16,
    pub query_port: Option<u16>,
    pub admin_port: Option<u16>,
    pub settings: Value,
    pub auto_restart: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidGenericConfig(pub String);

#[derive(Debug, Clone, Serialize)]
pub struct GenericGameInstance {
    #[serde(flatten)]
    pub identity: GameInstanceIdentity,
    #[serde(flatten)]
    pub config: GenericGameConfig,
    pub pid: Option<u32>,
    pub pid_started_at: Option<i64>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub last_stopped_at: Option<DateTime<Utc>>,
}

impl GenericGameInstance {
    pub fn is_running(&self) -> bool {
        matches!((self.pid, self.pid_started_at), (Some(pid), Some(started_at)) if crate::instance::process::is_alive(pid, started_at))
    }

    pub fn name(&self) -> &str {
        &self.identity.name
    }
}

pub fn is_generic_game(game: GameId) -> bool {
    matches!(
        game,
        GameId::VRising | GameId::Palworld | GameId::RunescapeDragonwilds | GameId::SevenDaysToDie
    )
}

pub fn default_generic_config(game: GameId, _name: &str) -> GenericGameConfig {
    match game {
        GameId::VRising => GenericGameConfig {
            port: 27015,
            query_port: Some(27016),
            admin_port: Some(25575),
            settings: Value::Object(Default::default()),
            auto_restart: false,
        },
        GameId::Palworld => GenericGameConfig {
            port: 8211,
            query_port: None,
            admin_port: Some(8212),
            settings: Value::Object(Default::default()),
            auto_restart: false,
        },
        GameId::RunescapeDragonwilds => GenericGameConfig {
            port: 7777,
            query_port: Some(8888),
            admin_port: None,
            settings: Value::Object(Default::default()),
            auto_restart: false,
        },
        GameId::SevenDaysToDie => GenericGameConfig {
            port: 26900,
            query_port: None,
            // 7D2D reserves its game UDP block at 26900–26902. Keep the
            // password-protected console on the immediately following port;
            // allocation moves the complete group together for later
            // instances.
            admin_port: Some(26903),
            settings: Value::Object(Default::default()),
            auto_restart: false,
        },
        _ => unreachable!("only generic games have generic defaults"),
    }
}

pub fn list_generic(db: &crate::db::Db, game: GameId) -> Result<Vec<GenericGameInstance>> {
    anyhow::ensure!(
        is_generic_game(game),
        "{game} does not use generic configuration"
    );
    let conn = db.conn();
    let mut statement = conn.prepare(
        "SELECT g.id, g.name, g.created_at, g.tags, c.port, c.query_port, c.admin_port, c.config_json, c.auto_restart, c.pid, c.pid_started_at, c.last_started_at, c.last_stopped_at \
         FROM game_instances g JOIN generic_game_instance_configs c ON c.instance_id = g.id \
         WHERE g.game = ?1 ORDER BY g.name",
    )?;
    statement
        .query_map(params![game.as_str()], |row| row_to_generic(row, game))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn load_generic(
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> Result<Option<GenericGameInstance>> {
    anyhow::ensure!(
        is_generic_game(game),
        "{game} does not use generic configuration"
    );
    let conn = db.conn();
    conn.query_row(
        "SELECT g.id, g.name, g.created_at, g.tags, c.port, c.query_port, c.admin_port, c.config_json, c.auto_restart, c.pid, c.pid_started_at, c.last_started_at, c.last_stopped_at \
         FROM game_instances g JOIN generic_game_instance_configs c ON c.instance_id = g.id \
         WHERE g.game = ?1 AND g.name = ?2",
        params![game.as_str(), name],
        |row| row_to_generic(row, game),
    ).optional().map_err(Into::into)
}

pub fn create_generic(
    paths: &Paths,
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> Result<GenericGameInstance> {
    anyhow::ensure!(
        is_generic_game(game),
        "{game} does not use generic configuration"
    );
    validate_instance_name(name).map_err(|error| anyhow::anyhow!(error))?;
    if load_generic(db, game, name)?.is_some() {
        bail!("{game} instance '{name}' already exists");
    }
    let mut config = default_generic_config(game, name);
    let occupied = configured_ports(db)?;
    while claimed_ports(game, &config)?
        .into_iter()
        .any(|port| occupied.contains(&port))
    {
        config.port = config
            .port
            .checked_add(10)
            .context("no port remains for game instance")?;
        config.query_port = config.query_port.map(|port| port.saturating_add(10));
        config.admin_port = config.admin_port.map(|port| port.saturating_add(10));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = Utc::now();
    if game == GameId::SevenDaysToDie {
        copy_seven_days_to_die_config(paths, name)?;
    } else {
        std::fs::create_dir_all(paths.game_instance_dir(game, name))?;
    }
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO game_instances (id, game, name, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, game.as_str(), name, created_at],
    )?;
    tx.execute("INSERT INTO generic_game_instance_configs (instance_id, port, query_port, admin_port, config_json, auto_restart) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![id, config.port, config.query_port, config.admin_port, serde_json::to_string(&config.settings)?, config.auto_restart])?;
    tx.commit()?;
    drop(conn);
    load_generic(db, game, name)?.context("failed to load newly-created game instance")
}

pub fn copy_seven_days_to_die_config(paths: &Paths, name: &str) -> Result<()> {
    let template = paths
        .game_install_dir(GameId::SevenDaysToDie)
        .join("serverconfig.xml");
    anyhow::ensure!(
        template.is_file(),
        "7 Days to Die is not installed (expected configuration template at {}); install it before creating an instance",
        template.display()
    );

    let destination = paths
        .game_instance_dir(GameId::SevenDaysToDie, name)
        .join("config/serverconfig.xml");
    anyhow::ensure!(
        !destination.exists(),
        "7 Days to Die configuration already exists at {}",
        destination.display()
    );
    let parent = destination
        .parent()
        .context("7 Days to Die configuration path has no parent")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    std::fs::copy(&template, &destination).with_context(|| {
        format!(
            "failed to copy 7 Days to Die configuration from {} to {}",
            template.display(),
            destination.display()
        )
    })?;
    std::fs::copy(&template, parent.join(SEVEN_DAYS_TEMPLATE_BASELINE_FILE)).with_context(
        || {
            format!(
                "failed to save 7 Days to Die configuration template from {}",
                template.display()
            )
        },
    )?;
    Ok(())
}

/// Every persisted port, including stopped servers. Allocation uses this to
/// avoid producing a configuration that will conflict as soon as another
/// instance is started.
pub fn configured_ports(db: &crate::db::Db) -> Result<HashSet<u16>> {
    let mut ports = HashSet::new();
    for instance in crate::db::instances::list_all(db)? {
        ports.extend(crate::game::ports::block(GameId::Valheim, instance.port)?);
    }
    for instance in list_rust(db)? {
        ports.extend([
            instance.config.port,
            instance.config.query_port,
            instance.config.rcon_port,
        ]);
    }
    for game in [
        GameId::VRising,
        GameId::Palworld,
        GameId::RunescapeDragonwilds,
        GameId::SevenDaysToDie,
    ] {
        for instance in list_generic(db, game)? {
            ports.extend(claimed_ports(game, &instance.config)?);
        }
    }
    Ok(ports)
}

pub fn claimed_ports(game: GameId, config: &GenericGameConfig) -> Result<Vec<u16>> {
    let mut ports = if game == GameId::SevenDaysToDie {
        crate::game::ports::block(game, config.port)?
    } else {
        vec![config.port]
    };
    ports.extend([config.query_port, config.admin_port].into_iter().flatten());
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

fn row_to_generic(row: &rusqlite::Row<'_>, game: GameId) -> rusqlite::Result<GenericGameInstance> {
    Ok(GenericGameInstance {
        identity: GameInstanceIdentity {
            id: row.get(0)?,
            game,
            name: row.get(1)?,
            created_at: row.get(2)?,
            tags: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
        },
        config: GenericGameConfig {
            port: row.get(4)?,
            query_port: row.get(5)?,
            admin_port: row.get(6)?,
            settings: serde_json::from_str(&row.get::<_, String>(7)?)
                .unwrap_or(Value::Object(Default::default())),
            auto_restart: row.get(8)?,
        },
        pid: row.get(9)?,
        pid_started_at: row.get(10)?,
        last_started_at: row.get(11)?,
        last_stopped_at: row.get(12)?,
    })
}

pub fn set_generic_pid(
    db: &crate::db::Db,
    game: GameId,
    name: &str,
    pid: u32,
    pid_started_at: i64,
    started_at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "UPDATE generic_game_instance_configs SET pid = ?3, pid_started_at = ?4, last_started_at = ?5 WHERE instance_id = (SELECT id FROM game_instances WHERE game = ?1 AND name = ?2)",
        params![game.as_str(), name, pid, pid_started_at, started_at],
    )?;
    Ok(())
}

pub fn clear_generic_pid(
    db: &crate::db::Db,
    game: GameId,
    name: &str,
    stopped_at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "UPDATE generic_game_instance_configs SET pid = NULL, pid_started_at = NULL, last_stopped_at = ?3 WHERE instance_id = (SELECT id FROM game_instances WHERE game = ?1 AND name = ?2)",
        params![game.as_str(), name, stopped_at],
    )?;
    Ok(())
}

pub fn delete_generic(db: &crate::db::Db, game: GameId, name: &str) -> Result<()> {
    db.conn().execute(
        "DELETE FROM game_instances WHERE game = ?1 AND name = ?2",
        params![game.as_str(), name],
    )?;
    Ok(())
}

pub fn set_generic_auto_restart(
    db: &crate::db::Db,
    game: GameId,
    name: &str,
    enabled: bool,
) -> Result<()> {
    db.conn().execute(
        "UPDATE generic_game_instance_configs SET auto_restart = ?3 WHERE instance_id = (SELECT id FROM game_instances WHERE game = ?1 AND name = ?2)",
        params![game.as_str(), name, enabled],
    )?;
    Ok(())
}

/// Changes a compiled generic driver's transport settings and its
/// game-owned configuration document. Configuration is deliberately only
/// mutable while stopped: Dragonwilds discards some live changes and the
/// other drivers do not provide an atomic live reload contract.
pub fn update_generic_config(
    db: &crate::db::Db,
    game: GameId,
    name: &str,
    config: &GenericGameConfig,
) -> Result<GenericGameInstance> {
    anyhow::ensure!(
        is_generic_game(game),
        "{game} does not use generic configuration"
    );
    let current = load_generic(db, game, name)?.context("game instance not found")?;
    if current.is_running() {
        bail!(crate::instance::InstanceError::AlreadyRunning(name.into()));
    }
    let persisted = GenericGameConfig {
        // Keep legacy JSON intact for compatibility, but game-owned files are
        // now the source of truth for these values.
        settings: current.config.settings,
        ..config.clone()
    };
    validate_generic_config(game, &persisted)?;
    db.conn().execute(
        "UPDATE generic_game_instance_configs SET port = ?3, query_port = ?4, admin_port = ?5, config_json = ?6, auto_restart = ?7 WHERE instance_id = (SELECT id FROM game_instances WHERE game = ?1 AND name = ?2)",
        params![game.as_str(), name, persisted.port, persisted.query_port, persisted.admin_port, serde_json::to_string(&persisted.settings)?, persisted.auto_restart],
    )?;
    load_generic(db, game, name)?.context("game instance disappeared while updating configuration")
}

pub fn validate_generic_config(game: GameId, config: &GenericGameConfig) -> Result<()> {
    let ports: Vec<_> = [Some(config.port), config.query_port, config.admin_port]
        .into_iter()
        .flatten()
        .collect();
    if ports.contains(&0) || ports.len() != ports.iter().collect::<HashSet<_>>().len() {
        bail!(InvalidGenericConfig(
            "game, query, and administration ports must be different and between 1 and 65535"
                .into()
        ));
    }
    if game == GameId::RunescapeDragonwilds && config.query_port.is_none() {
        bail!(InvalidGenericConfig(
            "RuneScape: Dragonwilds beacon port is required".into()
        ));
    }
    if game == GameId::SevenDaysToDie && config.admin_port.is_none() {
        bail!(InvalidGenericConfig(
            "7 Days to Die console port is required".into(),
        ));
    }
    Ok(())
}

pub fn identity(
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> Result<Option<GameInstanceIdentity>> {
    let conn = db.conn();
    let identity = conn
        .query_row(
            "SELECT id, created_at, tags FROM game_instances WHERE game = ?1 AND name = ?2",
            params![game.as_str(), name],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(identity.map(|(id, created_at, tags)| GameInstanceIdentity {
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        id,
        game,
        name: name.to_string(),
        created_at,
    }))
}

/// Resolves a stable instance UUID to the game and mutable display name it
/// currently belongs to. The hidden supervisor command uses this rather than
/// accepting a name from its parent process, so a rename cannot make an
/// already-spawned hand-off target the wrong instance.
pub fn identity_by_id(db: &crate::db::Db, id: &str) -> Result<Option<GameInstanceIdentity>> {
    let conn = db.conn();
    conn.query_row(
        "SELECT game, name, created_at, tags FROM game_instances WHERE id = ?1",
        params![id],
        |row| {
            let game: String = row.get(0)?;
            let game = game.parse().map_err(|error: String| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
                )
            })?;
            Ok(GameInstanceIdentity {
                id: id.to_string(),
                game,
                name: row.get(1)?,
                created_at: row.get(2)?,
                tags: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn valheim_identity(db: &crate::db::Db, name: &str) -> Result<GameInstanceIdentity> {
    identity(db, GameId::Valheim, name)?.context("Valheim instance is missing its game identity")
}

pub fn ensure_valheim_identity(
    db: &crate::db::Db,
    name: &str,
    created_at: DateTime<Utc>,
) -> Result<GameInstanceIdentity> {
    {
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO game_instances (id, game, name, created_at) VALUES (?1, 'valheim', ?2, ?3)",
            params![uuid::Uuid::new_v4().to_string(), name, created_at],
        )?;
    }
    valheim_identity(db, name)
}

pub fn list_rust(db: &crate::db::Db) -> Result<Vec<RustInstance>> {
    let conn = db.conn();
    let mut statement = conn.prepare(
        "SELECT g.id, g.name, g.created_at, r.port, r.query_port, r.rcon_port, r.rcon_password, r.hostname, r.level, r.seed, r.world_size, r.max_players, r.auto_restart, r.pid, r.pid_started_at, r.last_started_at, r.last_stopped_at, g.tags \
         FROM game_instances g JOIN rust_instance_configs r ON r.instance_id = g.id \
         WHERE g.game = 'rust' ORDER BY g.name",
    )?;
    statement
        .query_map([], row_to_rust)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn load_rust(db: &crate::db::Db, name: &str) -> Result<Option<RustInstance>> {
    let conn = db.conn();
    conn.query_row(
        "SELECT g.id, g.name, g.created_at, r.port, r.query_port, r.rcon_port, r.rcon_password, r.hostname, r.level, r.seed, r.world_size, r.max_players, r.auto_restart, r.pid, r.pid_started_at, r.last_started_at, r.last_stopped_at, g.tags \
         FROM game_instances g JOIN rust_instance_configs r ON r.instance_id = g.id \
         WHERE g.game = 'rust' AND g.name = ?1",
        params![name],
        row_to_rust,
    )
    .optional()
    .map_err(Into::into)
}

pub fn create_rust(paths: &Paths, db: &crate::db::Db, name: &str) -> Result<RustInstance> {
    validate_instance_name(name).map_err(|error| anyhow::anyhow!(error))?;
    if load_rust(db, name)?.is_some() {
        bail!("Rust instance '{name}' already exists");
    }
    let port = next_rust_port(db)?;
    let config = rust::default_config(name, port);
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = Utc::now();
    std::fs::create_dir_all(paths.game_instance_dir(GameId::Rust, name))?;
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO game_instances (id, game, name, created_at) VALUES (?1, 'rust', ?2, ?3)",
        params![id, name, created_at],
    )?;
    tx.execute(
        "INSERT INTO rust_instance_configs (instance_id, port, query_port, rcon_port, rcon_password, hostname, level, seed, world_size, max_players, auto_restart) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![id, config.port, config.query_port, config.rcon_port, config.rcon_password, config.hostname, config.level, config.seed, config.world_size, config.max_players, config.auto_restart],
    )?;
    tx.commit()?;
    drop(conn);
    let instance = load_rust(db, name)?.context("failed to load newly-created Rust instance")?;
    rust::ensure_layout(paths, &instance)?;
    Ok(instance)
}

pub fn update_rust_config(
    db: &crate::db::Db,
    name: &str,
    config: &RustInstanceConfig,
) -> Result<RustInstance> {
    let instance = load_rust(db, name)?.context("Rust instance not found")?;
    if instance.is_running() {
        bail!(crate::instance::InstanceError::AlreadyRunning(name.into()));
    }
    if config.port == 0
        || config.query_port == 0
        || config.rcon_port == 0
        || [config.port, config.query_port, config.rcon_port]
            .into_iter()
            .collect::<HashSet<_>>()
            .len()
            != 3
    {
        bail!(InvalidRustConfig(
            "Rust game, query, and RCON ports must be different and between 1 and 65535".into()
        ));
    }
    if config.rcon_password.trim().is_empty() {
        bail!(InvalidRustConfig(
            "Rust RCON password cannot be empty".into()
        ));
    }
    if config.hostname.trim().is_empty() || config.level.trim().is_empty() {
        bail!(InvalidRustConfig(
            "Rust hostname and level cannot be empty".into()
        ));
    }
    if config.world_size == 0 || config.max_players == 0 {
        bail!(InvalidRustConfig(
            "Rust world size and max players must be greater than zero".into()
        ));
    }

    db.conn().execute(
        "UPDATE rust_instance_configs SET hostname = ?2, level = ?3, seed = ?4, world_size = ?5, max_players = ?6, auto_restart = ?7, port = ?8, query_port = ?9, rcon_port = ?10, rcon_password = ?11 \
         WHERE instance_id = (SELECT id FROM game_instances WHERE game = 'rust' AND name = ?1)",
        params![name, config.hostname, config.level, config.seed, config.world_size, config.max_players, config.auto_restart, config.port, config.query_port, config.rcon_port, config.rcon_password],
    )?;
    load_rust(db, name)?.context("Rust instance disappeared while updating configuration")
}

pub fn set_rust_pid(
    db: &crate::db::Db,
    name: &str,
    pid: u32,
    pid_started_at: i64,
    started_at: DateTime<Utc>,
) -> Result<RustInstance> {
    db.conn().execute(
        "UPDATE rust_instance_configs SET pid = ?2, pid_started_at = ?3, last_started_at = ?4 WHERE instance_id = (SELECT id FROM game_instances WHERE game = 'rust' AND name = ?1)",
        params![name, pid, pid_started_at, started_at],
    )?;
    load_rust(db, name)?.context("Rust instance not found")
}

pub fn clear_rust_pid(db: &crate::db::Db, name: &str, stopped_at: DateTime<Utc>) -> Result<()> {
    db.conn().execute(
        "UPDATE rust_instance_configs SET pid = NULL, pid_started_at = NULL, last_stopped_at = ?2 WHERE instance_id = (SELECT id FROM game_instances WHERE game = 'rust' AND name = ?1)",
        params![name, stopped_at],
    )?;
    Ok(())
}

pub fn delete_rust(db: &crate::db::Db, name: &str) -> Result<()> {
    db.conn()
        .execute(
            "DELETE FROM game_instances WHERE game = 'rust' AND name = ?1",
            params![name],
        )
        .with_context(|| format!("failed to delete Rust instance '{name}'"))?;
    Ok(())
}

fn next_rust_port(db: &crate::db::Db) -> Result<u16> {
    let conn = db.conn();
    let mut reserved_ports = HashSet::new();

    // Valheim occupies the block declared by its compiled driver.
    // Rust owns both its game and query ports, which may not be consecutive
    // after configuration changes, so reserve their recorded values.
    let mut valheim_ports = conn.prepare("SELECT port FROM instances")?;
    for port in valheim_ports
        .query_map([], |row| row.get::<_, u16>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
    {
        reserved_ports.extend(crate::game::ports::block(GameId::Valheim, port)?);
    }
    let mut rust_ports =
        conn.prepare("SELECT port, query_port, rcon_port FROM rust_instance_configs")?;
    for (port, query_port, rcon_port) in rust_ports
        .query_map([], |row| {
            Ok((
                row.get::<_, u16>(0)?,
                row.get::<_, u16>(1)?,
                row.get::<_, u16>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    {
        reserved_ports.insert(port);
        reserved_ports.insert(query_port);
        reserved_ports.insert(rcon_port);
    }

    let mut port = 28015u16;
    loop {
        let candidate = crate::game::ports::block(GameId::Rust, port)?;
        if candidate.iter().all(|port| !reserved_ports.contains(port)) {
            return Ok(port);
        }
        port = port.checked_add(2).context("no Rust port block remains")?;
    }
}

fn row_to_rust(row: &rusqlite::Row<'_>) -> rusqlite::Result<RustInstance> {
    Ok(RustInstance {
        identity: GameInstanceIdentity {
            tags: serde_json::from_str(&row.get::<_, String>(17)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            id: row.get(0)?,
            game: GameId::Rust,
            name: row.get(1)?,
            created_at: row.get(2)?,
        },
        config: RustInstanceConfig {
            port: row.get(3)?,
            query_port: row.get(4)?,
            rcon_port: row.get(5)?,
            rcon_password: row.get(6)?,
            hostname: row.get(7)?,
            level: row.get(8)?,
            seed: row.get(9)?,
            world_size: row.get(10)?,
            max_players: row.get(11)?,
            auto_restart: row.get(12)?,
        },
        pid: row.get(13)?,
        pid_started_at: row.get(14)?,
        last_started_at: row.get(15)?,
        last_stopped_at: row.get(16)?,
    })
}

pub fn set_tags(db: &crate::db::Db, game: GameId, name: &str, tags: &[String]) -> Result<()> {
    anyhow::ensure!(
        tags.len() <= 20
            && tags.iter().all(|tag| !tag.is_empty()
                && tag.len() <= 32
                && tag.chars().all(|c| c.is_alphanumeric() || "-_".contains(c))),
        crate::instance::InstanceError::InvalidName(
            "Use at most 20 tags of 1–32 letters, digits, hyphens or underscores".into()
        )
    );
    let mut tags = tags.to_vec();
    tags.sort();
    tags.dedup();
    let count = db.conn().execute(
        "UPDATE game_instances SET tags = ?3 WHERE game = ?1 AND name = ?2",
        params![game.as_str(), name, serde_json::to_string(&tags)?],
    )?;
    anyhow::ensure!(
        count == 1,
        crate::instance::InstanceError::NotFound(name.into())
    );
    Ok(())
}

pub fn rename(db: &crate::db::Db, game: GameId, old: &str, new: &str) -> Result<()> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    tx.pragma_update(None, "defer_foreign_keys", true)?;
    let id: String = tx.query_row(
        "SELECT id FROM game_instances WHERE game = ?1 AND name = ?2",
        params![game.as_str(), old],
        |row| row.get(0),
    )?;
    tx.execute(
        "UPDATE game_instances SET name = ?2 WHERE id = ?1",
        params![id, new],
    )?;
    if game == GameId::Valheim {
        tx.execute(
            "UPDATE instances SET name = ?2 WHERE name = ?1",
            params![old, new],
        )?;
        for table in ["installed_mods", "access_list_entries"] {
            tx.execute(
                &format!("UPDATE {table} SET instance_name = ?2 WHERE instance_id = ?1"),
                params![id, new],
            )?;
        }
    }
    for table in [
        "backups",
        "backup_schedules",
        "backup_storage_configs",
        "resource_samples",
    ] {
        tx.execute(
            &format!("UPDATE {table} SET instance_name = ?2 WHERE instance_id = ?1"),
            params![id, new],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_context(label: &str) -> (Paths, crate::db::Db) {
        let dir = std::env::temp_dir().join(format!(
            "odin-rust-config-test-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = crate::db::Db::open(&paths).unwrap();
        (paths, db)
    }

    fn write_seven_days_template(paths: &Paths, contents: &str) {
        let template = paths
            .game_install_dir(GameId::SevenDaysToDie)
            .join("serverconfig.xml");
        std::fs::create_dir_all(template.parent().unwrap()).unwrap();
        std::fs::write(template, contents).unwrap();
    }

    #[test]
    fn update_rust_config_persists_game_specific_settings() {
        let (paths, db) = temp_context("settings");
        let instance = create_rust(&paths, &db, "rust-server").unwrap();
        assert!(!instance.config.auto_restart);
        let config = RustInstanceConfig {
            port: 29000,
            query_port: 30000,
            rcon_port: 31000,
            rcon_password: "rcon-secret".to_string(),
            hostname: "Rust Server".to_string(),
            level: "Barren".to_string(),
            seed: 42,
            world_size: 4000,
            max_players: 100,
            auto_restart: true,
        };

        let updated = update_rust_config(&db, "rust-server", &config).unwrap();

        assert_eq!(updated.config.hostname, "Rust Server");
        assert_eq!(updated.config.port, 29000);
        assert_eq!(updated.config.query_port, 30000);
        assert_eq!(updated.config.rcon_port, 31000);
        assert_eq!(updated.config.rcon_password, "rcon-secret");
        assert_eq!(updated.config.level, "Barren");
        assert_eq!(updated.config.seed, 42);
        assert_eq!(updated.config.world_size, 4000);
        assert_eq!(updated.config.max_players, 100);
        assert!(updated.config.auto_restart);

        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn identity_by_id_resolves_the_stable_game_identity() {
        let (paths, db) = temp_context("identity-by-id");
        let instance = create_rust(&paths, &db, "rust-server").unwrap();

        let resolved = identity_by_id(&db, &instance.identity.id).unwrap().unwrap();

        assert_eq!(resolved.id, instance.identity.id);
        assert_eq!(resolved.game, GameId::Rust);
        assert_eq!(resolved.name, "rust-server");
        assert!(identity_by_id(&db, "missing").unwrap().is_none());

        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn dragonwilds_operational_configuration_does_not_require_game_file_values() {
        let (paths, db) = temp_context("dragonwilds-config");
        let instance = create_generic(&paths, &db, GameId::RunescapeDragonwilds, "dragon").unwrap();
        let updated = update_generic_config(
            &db,
            GameId::RunescapeDragonwilds,
            "dragon",
            &instance.config,
        )
        .unwrap();
        assert!(updated.config.settings.as_object().unwrap().is_empty());
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn palworld_operational_configuration_preserves_legacy_game_values() {
        let (paths, db) = temp_context("palworld-config");
        let instance = create_generic(&paths, &db, GameId::Palworld, "pals").unwrap();
        let legacy = GenericGameConfig {
            settings: json!({
                "server_name": "Pals",
                "max_players": 32,
                "rest_api_enabled": true,
                "admin_password": "admin-secret",
                "server_password": "join-secret"
            }),
            ..instance.config
        };
        db.conn()
            .execute(
                "UPDATE generic_game_instance_configs SET config_json = ?1 WHERE instance_id = ?2",
                rusqlite::params![
                    serde_json::to_string(&legacy.settings).unwrap(),
                    instance.identity.id
                ],
            )
            .unwrap();
        let updated =
            update_generic_config(&db, GameId::Palworld, "pals", &instance.config).unwrap();

        assert_eq!(updated.config.settings["admin_password"], "admin-secret");
        assert_eq!(updated.config.settings["server_password"], "join-secret");
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn seven_days_operational_configuration_ignores_legacy_sandbox_code() {
        let (paths, db) = temp_context("7d2d-sandbox-code");
        write_seven_days_template(&paths, "<ServerSettings/>");
        let instance = create_generic(&paths, &db, GameId::SevenDaysToDie, "undead").unwrap();
        let updated =
            update_generic_config(&db, GameId::SevenDaysToDie, "undead", &instance.config).unwrap();
        assert!(updated.config.settings.as_object().unwrap().is_empty());
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn seven_days_creation_copies_the_installed_server_config() {
        let (paths, db) = temp_context("7d2d-config-copy");
        let template = "<?xml version=\"1.0\"?>\n<ServerSettings><property name=\"ServerName\" value=\"Default\"/></ServerSettings>\n";
        write_seven_days_template(&paths, template);

        create_generic(&paths, &db, GameId::SevenDaysToDie, "undead").unwrap();

        let config = paths
            .game_instance_dir(GameId::SevenDaysToDie, "undead")
            .join("config/serverconfig.xml");
        assert_eq!(std::fs::read_to_string(config).unwrap(), template);
        assert_eq!(
            std::fs::read_to_string(
                paths
                    .game_instance_dir(GameId::SevenDaysToDie, "undead")
                    .join(format!("config/{SEVEN_DAYS_TEMPLATE_BASELINE_FILE}")),
            )
            .unwrap(),
            template
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn seven_days_creation_requires_the_installed_server_config() {
        let (paths, db) = temp_context("7d2d-missing-config");

        let error = create_generic(&paths, &db, GameId::SevenDaysToDie, "undead")
            .unwrap_err()
            .to_string();

        assert!(error.contains("install it before creating an instance"));
        assert!(
            load_generic(&db, GameId::SevenDaysToDie, "undead")
                .unwrap()
                .is_none()
        );
        assert!(
            !paths
                .game_instance_dir(GameId::SevenDaysToDie, "undead")
                .exists()
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn rust_port_allocation_skips_valheim_port_blocks() {
        let (paths, db) = temp_context("ports");
        let mut valheim = crate::instance::Instance::create(&paths, &db, "valheim-server").unwrap();
        valheim.state.port = 28016;
        valheim.save(&db).unwrap();

        let rust = create_rust(&paths, &db, "rust-server").unwrap();

        assert_eq!(rust.config.port, 28019);
        assert_eq!(rust.config.query_port, 28020);
        assert_eq!(rust.config.rcon_port, 28021);

        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn port_allocation_reserves_independently_edited_rust_ports() {
        let (paths, db) = temp_context("edited-ports");
        let instance = create_rust(&paths, &db, "first").unwrap();
        let config = RustInstanceConfig {
            port: 28018,
            query_port: 28015,
            ..instance.config
        };
        update_rust_config(&db, "first", &config).unwrap();

        let second = create_rust(&paths, &db, "second").unwrap();
        assert_eq!(second.config.port, 28019);
        assert_eq!(second.config.query_port, 28020);
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }
}
