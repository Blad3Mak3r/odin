//! Game-neutral identity records plus Rust's v1 configuration.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::cli::validate_instance_name;
use crate::game::{GameId, rust};
use crate::paths::Paths;

#[derive(Debug, Clone, Serialize)]
pub struct GameInstanceIdentity {
    pub id: String,
    pub game: GameId,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RustInstanceConfig {
    pub port: u16,
    pub query_port: u16,
    pub hostname: String,
    pub level: String,
    pub seed: u32,
    pub world_size: u32,
    pub max_players: u16,
}

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

pub fn identity(
    db: &crate::db::Db,
    game: GameId,
    name: &str,
) -> Result<Option<GameInstanceIdentity>> {
    let conn = db.conn();
    let identity = conn
        .query_row(
            "SELECT id, created_at FROM game_instances WHERE game = ?1 AND name = ?2",
            params![game.as_str(), name],
            |row| Ok((row.get::<_, String>(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(identity.map(|(id, created_at)| GameInstanceIdentity {
        id,
        game,
        name: name.to_string(),
        created_at,
    }))
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
        "SELECT g.id, g.name, g.created_at, r.port, r.query_port, r.hostname, r.level, r.seed, r.world_size, r.max_players, r.pid, r.pid_started_at, r.last_started_at, r.last_stopped_at \
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
        "SELECT g.id, g.name, g.created_at, r.port, r.query_port, r.hostname, r.level, r.seed, r.world_size, r.max_players, r.pid, r.pid_started_at, r.last_started_at, r.last_stopped_at \
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
        "INSERT INTO rust_instance_configs (instance_id, port, query_port, hostname, level, seed, world_size, max_players) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, config.port, config.query_port, config.hostname, config.level, config.seed, config.world_size, config.max_players],
    )?;
    tx.commit()?;
    drop(conn);
    load_rust(db, name)?.context("failed to load newly-created Rust instance")
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

fn next_rust_port(db: &crate::db::Db) -> Result<u16> {
    let conn = db.conn();
    let mut statement = conn.prepare("SELECT port FROM rust_instance_configs")?;
    let ports = statement
        .query_map([], |row| row.get::<_, u16>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut port = 28015u16;
    while ports.contains(&port) {
        port = port.checked_add(2).context("no Rust port block remains")?;
    }
    Ok(port)
}

fn row_to_rust(row: &rusqlite::Row<'_>) -> rusqlite::Result<RustInstance> {
    Ok(RustInstance {
        identity: GameInstanceIdentity {
            id: row.get(0)?,
            game: GameId::Rust,
            name: row.get(1)?,
            created_at: row.get(2)?,
        },
        config: RustInstanceConfig {
            port: row.get(3)?,
            query_port: row.get(4)?,
            hostname: row.get(5)?,
            level: row.get(6)?,
            seed: row.get(7)?,
            world_size: row.get(8)?,
            max_players: row.get(9)?,
        },
        pid: row.get(10)?,
        pid_started_at: row.get(11)?,
        last_started_at: row.get(12)?,
        last_stopped_at: row.get(13)?,
    })
}
