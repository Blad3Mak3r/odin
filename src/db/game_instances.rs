//! Game-neutral identity records plus Rust's v1 configuration.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::cli::validate_instance_name;
use crate::game::{GameId, ark_asa, cs2, rust};
use crate::paths::Paths;

pub(super) fn migrate_native_configuration(paths: &Paths, db: &crate::db::Db) -> Result<()> {
    let mut conn = db.conn();
    let staged: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'rust_file_config_migrations')",
        [],
        |row| row.get(0),
    )?;
    if !staged {
        return Ok(());
    }

    let rust_rows = {
        let mut statement = conn.prepare(
            "SELECT g.name, m.hostname, m.level, m.seed, m.world_size, m.max_players \
             FROM rust_file_config_migrations m
             JOIN game_instances g ON g.id = m.instance_id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    crate::game::rust::RustFileConfig {
                        hostname: row.get(1)?,
                        level: row.get(2)?,
                        seed: row.get(3)?,
                        world_size: row.get(4)?,
                        max_players: row.get(5)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (name, config) in rust_rows {
        let path = paths
            .game_instance_dir(GameId::Rust, &name)
            .join("server/cfg/server.cfg");
        crate::game::config_documents::migrate_rust_file_config(&path, &config)
            .with_context(|| format!("failed to migrate Rust configuration for '{name}'"))?;
    }

    let legacy_json = {
        let mut statement = conn.prepare(
            "SELECT g.game, g.name, m.config_json \
             FROM generic_config_json_migrations m JOIN game_instances g ON g.id = m.instance_id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (game, name, contents) in legacy_json {
        let game: GameId = game.parse().map_err(anyhow::Error::msg)?;
        let path = paths
            .game_instance_dir(game, &name)
            .join("legacy-config.json.disabled");
        write_private_backup(&path, &contents)?;
    }

    let tx = conn.transaction()?;
    tx.execute_batch(
        "CREATE TABLE rust_instance_configs_next (
            instance_id TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
            port INTEGER NOT NULL, query_port INTEGER NOT NULL,
            rcon_port INTEGER NOT NULL, rcon_password TEXT NOT NULL,
            auto_restart INTEGER NOT NULL DEFAULT 0
         );
         INSERT INTO rust_instance_configs_next
            SELECT instance_id, port, query_port, rcon_port, rcon_password, auto_restart
            FROM rust_instance_configs;
         DROP TABLE rust_instance_configs;
         ALTER TABLE rust_instance_configs_next RENAME TO rust_instance_configs;

         CREATE TABLE generic_game_instance_configs_next (
            instance_id TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
            port INTEGER NOT NULL, query_port INTEGER, admin_port INTEGER,
            auto_restart INTEGER NOT NULL DEFAULT 0
         );
         INSERT INTO generic_game_instance_configs_next
            SELECT c.instance_id, c.port, c.query_port,
                   CASE WHEN g.game = 'palworld' THEN NULL ELSE c.admin_port END,
                   c.auto_restart
            FROM generic_game_instance_configs c
            JOIN game_instances g ON g.id = c.instance_id;
         DROP TABLE generic_game_instance_configs;
         ALTER TABLE generic_game_instance_configs_next RENAME TO generic_game_instance_configs;
         DROP TABLE rust_file_config_migrations;
         DROP TABLE generic_config_json_migrations;",
    )?;
    tx.commit()?;
    Ok(())
}

fn write_private_backup(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        let existing = fs::read_to_string(path)?;
        anyhow::ensure!(
            existing == contents,
            "legacy configuration backup already exists with different contents at {}",
            path.display()
        );
        return Ok(());
    }
    let parent = path
        .parent()
        .context("legacy configuration backup has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".legacy-config-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
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
}

impl RustInstance {
    pub fn name(&self) -> &str {
        &self.identity.name
    }
}

/// Process arguments and operational metadata for a compiled generic driver.
/// Game settings live exclusively in the native configuration documents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericGameConfig {
    pub port: u16,
    pub query_port: Option<u16>,
    pub admin_port: Option<u16>,
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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallMode {
    Isolated,
    Shared,
}

impl InstallMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::Shared => "shared",
        }
    }
}

impl GenericGameInstance {
    pub fn name(&self) -> &str {
        &self.identity.name
    }
}

pub fn is_generic_game(game: GameId) -> bool {
    matches!(
        game,
        GameId::ArkSurvivalAscended | GameId::Palworld | GameId::SevenDaysToDie | GameId::Cs2
    )
}

/// Every game that keeps its configuration in the generic tables.
pub fn generic_games() -> impl Iterator<Item = GameId> {
    crate::game::drivers()
        .into_iter()
        .map(|driver| driver.id())
        .filter(|game| is_generic_game(*game))
}

pub fn default_generic_config(game: GameId, _name: &str) -> GenericGameConfig {
    match game {
        GameId::ArkSurvivalAscended => GenericGameConfig {
            port: 7777,
            query_port: Some(27015),
            admin_port: Some(27020),
            auto_restart: false,
        },
        GameId::Palworld => GenericGameConfig {
            port: 8211,
            // Palworld's Steam server browser listener otherwise defaults to
            // 27015 for every process. It is a launch argument, rather than
            // an INI setting, and must be allocated per instance.
            query_port: Some(27015),
            admin_port: None,
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
            auto_restart: false,
        },
        GameId::Cs2 => GenericGameConfig {
            port: 27015,
            // Source RCON shares the TCP game port, and the SourceTV port is
            // derived from the game port, so neither is configurable.
            query_port: None,
            admin_port: None,
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
        "SELECT g.id, g.name, g.created_at, g.tags, c.port, c.query_port, c.admin_port, c.auto_restart \
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
        "SELECT g.id, g.name, g.created_at, g.tags, c.port, c.query_port, c.admin_port, c.auto_restart \
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
    std::fs::create_dir_all(paths.game_instance_dir(game, name))?;
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO game_instances (id, game, name, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, game.as_str(), name, created_at],
    )?;
    tx.execute("INSERT INTO generic_game_instance_configs (instance_id, port, query_port, admin_port, auto_restart) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, config.port, config.query_port, config.admin_port, config.auto_restart])?;
    if game == GameId::ArkSurvivalAscended {
        let options = ark_asa::ArkLaunchOptions::defaults(name);
        tx.execute(
            "INSERT INTO ark_launch_options (instance_id, map, session_name, mods, rcon_password) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, options.map, options.session_name, serde_json::to_string(&options.mods)?, uuid::Uuid::new_v4().to_string()],
        )?;
    }
    if game == GameId::Cs2 {
        let options = cs2::Cs2LaunchOptions::defaults();
        tx.execute(
            "INSERT INTO cs2_launch_options (instance_id, map, game_mode, max_players, gslt, rcon_password) VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
            params![id, options.map, options.game_mode.as_str(), options.max_players, uuid::Uuid::new_v4().to_string()],
        )?;
    }
    tx.commit()?;
    drop(conn);
    load_generic(db, game, name)?.context("failed to load newly-created game instance")
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
    for game in generic_games() {
        for instance in list_generic(db, game)? {
            ports.extend(claimed_ports(game, &instance.config)?);
        }
    }
    Ok(ports)
}

pub fn claimed_ports(game: GameId, config: &GenericGameConfig) -> Result<Vec<u16>> {
    let mut ports = if game == GameId::SevenDaysToDie {
        crate::game::ports::block(game, config.port)?
    } else if game == GameId::Cs2 {
        vec![config.port, cs2::tv_port(config.port)?]
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
            auto_restart: row.get(7)?,
        },
    })
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
/// game-owned configuration document. Callers must only change configuration
/// while the server is stopped: the drivers do not provide an atomic live reload
/// contract.
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
    load_generic(db, game, name)?.context("game instance not found")?;
    let persisted = config.clone();
    validate_generic_config(game, &persisted)?;
    db.conn().execute(
        "UPDATE generic_game_instance_configs SET port = ?3, query_port = ?4, admin_port = ?5, auto_restart = ?6 WHERE instance_id = (SELECT id FROM game_instances WHERE game = ?1 AND name = ?2)",
        params![game.as_str(), name, persisted.port, persisted.query_port, persisted.admin_port, persisted.auto_restart],
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
    if game == GameId::Palworld && config.query_port.is_none() {
        bail!(InvalidGenericConfig(
            "Palworld Steam query port is required".into()
        ));
    }
    if game == GameId::Palworld && config.admin_port.is_some() {
        bail!(InvalidGenericConfig(
            "Palworld REST API port belongs in PalWorldSettings.ini".into()
        ));
    }
    if game == GameId::SevenDaysToDie && config.admin_port.is_none() {
        bail!(InvalidGenericConfig(
            "7 Days to Die console port is required".into(),
        ));
    }
    if game == GameId::Cs2 && (config.query_port.is_some() || config.admin_port.is_some()) {
        bail!(InvalidGenericConfig(
            "Counter-Strike 2 has no separate query or RCON port: both use the game port".into()
        ));
    }
    if game == GameId::Cs2 {
        cs2::tv_port(config.port).map_err(|error| InvalidGenericConfig(error.to_string()))?;
    }
    if game == GameId::ArkSurvivalAscended
        && (config.query_port.is_none() || config.admin_port.is_none())
    {
        bail!(InvalidGenericConfig(
            "ARK query and RCON ports are required".into(),
        ));
    }
    Ok(())
}

pub fn install_mode(db: &crate::db::Db, instance_id: &str) -> Result<InstallMode> {
    let mode = db
        .conn()
        .query_row(
            "SELECT install_mode FROM game_instance_install WHERE instance_id = ?1",
            params![instance_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match mode.as_deref() {
        None | Some("isolated") => Ok(InstallMode::Isolated),
        Some("shared") => Ok(InstallMode::Shared),
        Some(mode) => bail!("invalid install mode '{mode}'"),
    }
}

pub fn is_shared_install(db: &crate::db::Db, instance_id: &str) -> Result<bool> {
    Ok(install_mode(db, instance_id)? == InstallMode::Shared)
}

/// Every instance of `game` that runs from the game's shared installation.
pub fn shared_install_instances(
    db: &crate::db::Db,
    game: GameId,
) -> Result<Vec<GameInstanceIdentity>> {
    let conn = db.conn();
    let mut statement = conn.prepare(
        "SELECT g.id, g.name, g.created_at, g.tags \
         FROM game_instances g JOIN game_instance_install i ON i.instance_id = g.id \
         WHERE g.game = ?1 AND i.install_mode = 'shared' ORDER BY g.name",
    )?;
    statement
        .query_map(params![game.as_str()], |row| {
            Ok(GameInstanceIdentity {
                id: row.get(0)?,
                game,
                name: row.get(1)?,
                created_at: row.get(2)?,
                tags: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn set_install_mode(db: &crate::db::Db, instance_id: &str, mode: InstallMode) -> Result<()> {
    db.conn().execute(
        "INSERT INTO game_instance_install (instance_id, install_mode) VALUES (?1, ?2)
         ON CONFLICT(instance_id) DO UPDATE SET install_mode = excluded.install_mode",
        params![instance_id, mode.as_str()],
    )?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ArkCluster {
    pub id: String,
    pub name: String,
    pub cluster_id: String,
    pub members: Vec<String>,
}

fn validate_ark_cluster_text(value: &str, field: &str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "ARK cluster {field} must contain 1 to 64 ASCII letters, numbers, - or _"
    );
    Ok(())
}

pub fn list_ark_clusters(db: &crate::db::Db) -> Result<Vec<ArkCluster>> {
    let connection = db.conn();
    let mut statement =
        connection.prepare("SELECT id, name, cluster_id FROM ark_clusters ORDER BY name")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (id, name, cluster_id) = row?;
        let mut members = connection.prepare("SELECT instance_id FROM ark_cluster_members WHERE cluster_id = ?1 ORDER BY instance_id")?;
        Ok(ArkCluster { members: members.query_map(params![id], |row| row.get(0))?.collect::<std::result::Result<_, _>>()?, id, name, cluster_id })
    }).collect()
}

pub fn create_ark_cluster(db: &crate::db::Db, name: &str, cluster_id: &str) -> Result<ArkCluster> {
    validate_ark_cluster_text(name, "name")?;
    validate_ark_cluster_text(cluster_id, "ID")?;
    let cluster = ArkCluster {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        cluster_id: cluster_id.into(),
        members: Vec::new(),
    };
    db.conn().execute(
        "INSERT INTO ark_clusters (id, name, cluster_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            cluster.id,
            cluster.name,
            cluster.cluster_id,
            chrono::Utc::now()
        ],
    )?;
    Ok(cluster)
}

pub fn set_ark_cluster_members(
    db: &crate::db::Db,
    cluster_id: &str,
    members: &[String],
) -> Result<()> {
    let connection = db.conn();
    let transaction = connection.unchecked_transaction()?;
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM ark_clusters WHERE id = ?1)",
        params![cluster_id],
        |row| row.get(0),
    )?;
    anyhow::ensure!(exists, "ARK cluster does not exist");
    for member in members {
        let game: String = transaction.query_row(
            "SELECT game FROM game_instances WHERE id = ?1",
            params![member],
            |row| row.get(0),
        )?;
        anyhow::ensure!(
            game == GameId::ArkSurvivalAscended.as_str(),
            "only ARK instances can join a cluster"
        );
    }
    transaction.execute(
        "DELETE FROM ark_cluster_members WHERE cluster_id = ?1",
        params![cluster_id],
    )?;
    for member in members {
        transaction.execute(
            "DELETE FROM ark_cluster_members WHERE instance_id = ?1",
            params![member],
        )?;
        transaction.execute(
            "INSERT INTO ark_cluster_members (instance_id, cluster_id) VALUES (?1, ?2)",
            params![member, cluster_id],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn delete_ark_cluster(db: &crate::db::Db, cluster_id: &str) -> Result<()> {
    db.conn().execute(
        "DELETE FROM ark_clusters WHERE id = ?1",
        params![cluster_id],
    )?;
    Ok(())
}

pub fn ark_cluster(db: &crate::db::Db, instance_id: &str) -> Result<Option<ArkCluster>> {
    let cluster = db.conn().query_row(
        "SELECT c.id, c.name, c.cluster_id FROM ark_clusters c JOIN ark_cluster_members m ON m.cluster_id = c.id WHERE m.instance_id = ?1",
        params![instance_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
    ).optional()?;
    cluster.map(|(id, name, cluster_id)| {
        let connection = db.conn();
        let mut statement = connection.prepare("SELECT instance_id FROM ark_cluster_members WHERE cluster_id = ?1 ORDER BY instance_id")?;
        let members = statement.query_map(params![id], |row| row.get(0))?.collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(ArkCluster { id, name, cluster_id, members })
    }).transpose()
}

pub fn ark_launch(
    db: &crate::db::Db,
    instance: &GenericGameInstance,
) -> Result<ark_asa::ArkLaunch> {
    anyhow::ensure!(
        instance.identity.game == GameId::ArkSurvivalAscended,
        "not an ARK instance"
    );
    let options = {
        let connection = db.conn();
        connection.query_row(
            "SELECT map, session_name, mods, rcon_password FROM ark_launch_options WHERE instance_id = ?1",
            params![instance.identity.id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)),
        ).optional()?
    };
    let (map, session_name, mods, rcon_password) =
        options.context("ARK instance is missing launch options")?;
    let cluster = ark_cluster(db, &instance.identity.id)?;
    Ok(ark_asa::ArkLaunch {
        name: instance.name().to_string(),
        port: instance.config.port,
        query_port: instance
            .config
            .query_port
            .context("ARK query port is required")?,
        rcon_port: instance
            .config
            .admin_port
            .context("ARK RCON port is required")?,
        rcon_password,
        options: ark_asa::ArkLaunchOptions {
            map,
            session_name,
            mods: serde_json::from_str(&mods)?,
        },
        cluster: cluster.map(|cluster| ark_asa::ArkClusterLaunch {
            directory: format!("/srv/odin/clusters/{}", cluster.cluster_id),
            cluster_id: cluster.cluster_id,
        }),
    })
}

pub fn update_ark_launch_options(
    db: &crate::db::Db,
    instance: &GenericGameInstance,
    options: &ark_asa::ArkLaunchOptions,
) -> Result<()> {
    options.validate()?;
    anyhow::ensure!(
        instance.identity.game == GameId::ArkSurvivalAscended,
        "not an ARK instance"
    );
    db.conn().execute(
        "UPDATE ark_launch_options SET map = ?2, session_name = ?3, mods = ?4 WHERE instance_id = ?1",
        params![instance.identity.id, options.map, options.session_name, serde_json::to_string(&options.mods)?],
    )?;
    Ok(())
}

pub fn cs2_launch(db: &crate::db::Db, instance: &GenericGameInstance) -> Result<cs2::Cs2Launch> {
    anyhow::ensure!(
        instance.identity.game == GameId::Cs2,
        "not a Counter-Strike 2 instance"
    );
    let row = {
        let connection = db.conn();
        connection
            .query_row(
                "SELECT map, game_mode, max_players, gslt, rcon_password FROM cs2_launch_options WHERE instance_id = ?1",
                params![instance.identity.id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u8>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
    };
    let (map, game_mode, max_players, gslt, rcon_password) =
        row.context("Counter-Strike 2 instance is missing launch options")?;
    Ok(cs2::Cs2Launch {
        name: instance.name().to_string(),
        port: instance.config.port,
        tv_port: cs2::tv_port(instance.config.port)?,
        rcon_password,
        options: cs2::Cs2LaunchOptions {
            map,
            game_mode: cs2::Cs2GameMode::parse(&game_mode)?,
            max_players,
            gslt,
        },
    })
}

pub fn update_cs2_launch_options(
    db: &crate::db::Db,
    instance: &GenericGameInstance,
    options: &cs2::Cs2LaunchOptions,
) -> Result<()> {
    options.validate()?;
    anyhow::ensure!(
        instance.identity.game == GameId::Cs2,
        "not a Counter-Strike 2 instance"
    );
    db.conn().execute(
        "UPDATE cs2_launch_options SET map = ?2, game_mode = ?3, max_players = ?4, gslt = ?5 WHERE instance_id = ?1",
        params![instance.identity.id, options.map, options.game_mode.as_str(), options.max_players, options.gslt],
    )?;
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
        "SELECT g.id, g.name, g.created_at, r.port, r.query_port, r.rcon_port, r.rcon_password, r.auto_restart, g.tags \
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
        "SELECT g.id, g.name, g.created_at, r.port, r.query_port, r.rcon_port, r.rcon_password, r.auto_restart, g.tags \
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
    let config = rust::default_config(port);
    let file_config = rust::default_file_config(name);
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
        "INSERT INTO rust_instance_configs (instance_id, port, query_port, rcon_port, rcon_password, auto_restart) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, config.port, config.query_port, config.rcon_port, config.rcon_password, config.auto_restart],
    )?;
    let pending = RustInstance {
        identity: GameInstanceIdentity {
            tags: Vec::new(),
            id,
            game: GameId::Rust,
            name: name.to_string(),
            created_at,
        },
        config,
    };
    rust::ensure_layout(paths, &pending)?;
    crate::game::config_documents::initialize_rust(paths, &pending, &file_config)?;
    tx.commit()?;
    drop(conn);
    load_rust(db, name)?.context("failed to load newly-created Rust instance")
}

pub fn update_rust_config(
    db: &crate::db::Db,
    name: &str,
    config: &RustInstanceConfig,
) -> Result<RustInstance> {
    load_rust(db, name)?.context("Rust instance not found")?;
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
    db.conn().execute(
        "UPDATE rust_instance_configs SET auto_restart = ?2, port = ?3, query_port = ?4, rcon_port = ?5, rcon_password = ?6 \
         WHERE instance_id = (SELECT id FROM game_instances WHERE game = 'rust' AND name = ?1)",
        params![name, config.auto_restart, config.port, config.query_port, config.rcon_port, config.rcon_password],
    )?;
    load_rust(db, name)?.context("Rust instance disappeared while updating configuration")
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
            tags: serde_json::from_str(&row.get::<_, String>(8)?)
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
            auto_restart: row.get(7)?,
        },
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
    fn update_rust_config_persists_operational_settings() {
        let (paths, db) = temp_context("settings");
        let instance = create_rust(&paths, &db, "rust-server").unwrap();
        assert!(!instance.config.auto_restart);
        let config = RustInstanceConfig {
            port: 29000,
            query_port: 30000,
            rcon_port: 31000,
            rcon_password: "rcon-secret".to_string(),
            auto_restart: true,
        };

        let updated = update_rust_config(&db, "rust-server", &config).unwrap();

        assert_eq!(updated.config.port, 29000);
        assert_eq!(updated.config.query_port, 30000);
        assert_eq!(updated.config.rcon_port, 31000);
        assert_eq!(updated.config.rcon_password, "rcon-secret");
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
    fn shared_install_instances_lists_only_shared_instances_of_the_game() {
        let (paths, db) = temp_context("shared-install-instances");
        let shared = create_generic(&paths, &db, GameId::Palworld, "shared").unwrap();
        let isolated = create_generic(&paths, &db, GameId::Palworld, "isolated").unwrap();
        let other_game = create_generic(&paths, &db, GameId::SevenDaysToDie, "other").unwrap();
        set_install_mode(&db, &shared.identity.id, InstallMode::Shared).unwrap();
        set_install_mode(&db, &isolated.identity.id, InstallMode::Isolated).unwrap();
        set_install_mode(&db, &other_game.identity.id, InstallMode::Shared).unwrap();

        let listed = shared_install_instances(&db, GameId::Palworld).unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, shared.identity.id);
        assert!(is_shared_install(&db, &shared.identity.id).unwrap());
        assert!(!is_shared_install(&db, &isolated.identity.id).unwrap());

        // An existing instance can change mode; the new mode replaces the old.
        set_install_mode(&db, &isolated.identity.id, InstallMode::Shared).unwrap();
        assert!(is_shared_install(&db, &isolated.identity.id).unwrap());
        assert_eq!(
            shared_install_instances(&db, GameId::Palworld)
                .unwrap()
                .len(),
            2
        );

        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn ark_cluster_membership_round_trips_and_derives_launch_data() {
        let (paths, db) = temp_context("ark-cluster");
        let instance = match crate::game::instances::create(
            &paths,
            &db,
            GameId::ArkSurvivalAscended,
            "island",
        )
        .unwrap()
        {
            crate::game::instances::GameInstance::Generic(instance) => instance,
            _ => unreachable!(),
        };
        let cluster = create_ark_cluster(&db, "home", "home_cluster").unwrap();
        set_ark_cluster_members(
            &db,
            &cluster.id,
            std::slice::from_ref(&instance.identity.id),
        )
        .unwrap();

        let loaded = ark_cluster(&db, &instance.identity.id).unwrap().unwrap();
        let launch = ark_launch(&db, &instance).unwrap();

        assert_eq!(loaded.cluster_id, "home_cluster");
        assert_eq!(
            launch.cluster.unwrap().directory,
            "/srv/odin/clusters/home_cluster"
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn ark_instance_can_move_between_clusters() {
        let (paths, db) = temp_context("ark-cluster-move");
        let instance = match crate::game::instances::create(
            &paths,
            &db,
            GameId::ArkSurvivalAscended,
            "island",
        )
        .unwrap()
        {
            crate::game::instances::GameInstance::Generic(instance) => instance,
            _ => unreachable!(),
        };
        let first = create_ark_cluster(&db, "first", "first_cluster").unwrap();
        let second = create_ark_cluster(&db, "second", "second_cluster").unwrap();
        set_ark_cluster_members(&db, &first.id, std::slice::from_ref(&instance.identity.id))
            .unwrap();
        set_ark_cluster_members(&db, &second.id, std::slice::from_ref(&instance.identity.id))
            .unwrap();

        assert_eq!(
            ark_cluster(&db, &instance.identity.id).unwrap().unwrap().id,
            second.id
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn cs2_instances_claim_the_game_and_derived_sourcetv_ports() {
        let (paths, db) = temp_context("cs2-ports");
        let first = create_generic(&paths, &db, GameId::Cs2, "dust").unwrap();
        let second = create_generic(&paths, &db, GameId::Cs2, "mirage").unwrap();

        assert_eq!(first.config.port, 27015);
        assert_eq!(first.config.query_port, None);
        assert_eq!(first.config.admin_port, None);
        assert_eq!(
            claimed_ports(GameId::Cs2, &first.config).unwrap(),
            [27015, 27020]
        );
        // The next instance moves the whole group, so SourceTV never repeats.
        assert_eq!(
            claimed_ports(GameId::Cs2, &second.config).unwrap(),
            [27025, 27030]
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn cs2_launch_options_start_with_defaults_and_are_editable() {
        let (paths, db) = temp_context("cs2-launch");
        let instance = create_generic(&paths, &db, GameId::Cs2, "dust").unwrap();

        let launch = cs2_launch(&db, &instance).unwrap();
        assert_eq!(launch.name, "dust");
        assert_eq!(launch.port, 27015);
        assert_eq!(launch.tv_port, 27020);
        assert_eq!(launch.options, cs2::Cs2LaunchOptions::defaults());
        assert!(!launch.rcon_password.is_empty());

        let options = cs2::Cs2LaunchOptions {
            map: "de_inferno".into(),
            game_mode: cs2::Cs2GameMode::Wingman,
            max_players: 8,
            gslt: Some("ABCDEF0123456789".into()),
        };
        update_cs2_launch_options(&db, &instance, &options).unwrap();
        let edited = cs2_launch(&db, &instance).unwrap();
        assert_eq!(edited.options, options);
        // Editing launch options never rotates the RCON password.
        assert_eq!(edited.rcon_password, launch.rcon_password);
        assert!(
            update_cs2_launch_options(
                &db,
                &instance,
                &cs2::Cs2LaunchOptions {
                    map: "bad map".into(),
                    ..options
                }
            )
            .is_err()
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn cs2_has_no_separate_query_or_rcon_port() {
        let config = GenericGameConfig {
            port: 27015,
            query_port: None,
            admin_port: None,
            auto_restart: false,
        };
        assert!(validate_generic_config(GameId::Cs2, &config).is_ok());
        for config in [
            GenericGameConfig {
                query_port: Some(27016),
                ..config.clone()
            },
            GenericGameConfig {
                admin_port: Some(27017),
                ..config.clone()
            },
            GenericGameConfig {
                port: u16::MAX,
                ..config
            },
        ] {
            assert!(validate_generic_config(GameId::Cs2, &config).is_err());
        }
    }

    #[test]
    fn palworld_operational_configuration_has_no_admin_port() {
        let (paths, db) = temp_context("palworld-config");
        let template = paths
            .game_install_dir(GameId::Palworld)
            .join("DefaultPalWorldSettings.ini");
        std::fs::create_dir_all(template.parent().unwrap()).unwrap();
        std::fs::write(&template, "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(PublicPort=8211,RESTAPIPort=8212)\n").unwrap();
        let instance = create_generic(&paths, &db, GameId::Palworld, "pals").unwrap();
        assert_eq!(instance.config.query_port, Some(27015));
        assert_eq!(instance.config.admin_port, None);
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn seven_days_operational_configuration_ignores_legacy_sandbox_code() {
        let (paths, db) = temp_context("7d2d-sandbox-code");
        write_seven_days_template(&paths, "<ServerSettings/>");
        let instance = create_generic(&paths, &db, GameId::SevenDaysToDie, "undead").unwrap();
        let updated =
            update_generic_config(&db, GameId::SevenDaysToDie, "undead", &instance.config).unwrap();
        assert_eq!(updated.config.admin_port, Some(26903));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn seven_days_creation_needs_neither_the_game_nor_a_config_file() {
        let (paths, db) = temp_context("7d2d-offline-create");

        create_generic(&paths, &db, GameId::SevenDaysToDie, "undead").unwrap();

        // The server seeds its native document from its own private install
        // when it first starts; creation never writes it.
        let root = paths.game_instance_dir(GameId::SevenDaysToDie, "undead");
        assert!(root.is_dir());
        assert!(!root.join("config/serverconfig.xml").exists());
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
