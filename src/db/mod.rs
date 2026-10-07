//! Odin's SQLite-backed persistence layer.
//!
//! A single `<data_dir>/odin.db`, opened in WAL mode so `odin serve` (a
//! long-lived process) and one-off CLI commands can safely run against the
//! same data dir concurrently — WAL allows concurrent readers alongside a
//! single writer, and `busy_timeout` makes a writer *wait* for its turn
//! instead of failing outright when another connection briefly holds the
//! write lock.
//!
//! Each data domain gets its own thin repository module (`instances`,
//! `mods`, `lists`, `backups`, `activity`, `cache`, `settings`) that takes
//! `&Db` and exposes calls shaped like the file-I/O functions they replace.

pub mod activity;
pub mod backup_schedules;
pub mod backup_storage;
pub mod backups;
pub mod cache;
pub mod game_instances;
pub mod global_mods;
mod import;
pub mod instances;
pub mod jobs;
pub mod lists;
mod migrations;
pub mod player_sessions;
pub mod resource_limits;
pub mod resource_samples;
pub mod settings;
pub mod uptime_schedules;
pub mod webhooks;

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::paths::Paths;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Opens (creating if needed) `<data_dir>/odin.db`, applies any pending
    /// migrations, and — on a completely fresh database — imports state
    /// from an existing file-based installation, if one is found.
    pub fn open(paths: &Paths) -> Result<Self> {
        std::fs::create_dir_all(&paths.data_dir)
            .with_context(|| format!("failed to create data dir {}", paths.data_dir.display()))?;
        Self::open_at(&paths.data_dir.join("odin.db"), paths)
    }

    fn open_at(db_path: &Path, paths: &Paths) -> Result<Self> {
        let mut conn = Connection::open(db_path)
            .with_context(|| format!("failed to open database {}", db_path.display()))?;
        apply_pragmas(&conn)?;
        migrations::backup_before_game_instances(&conn, db_path)?;
        migrations::run(&mut conn)?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        import::bootstrap_if_empty(&db, paths)?;
        game_instances::migrate_native_configuration(paths, &db)?;
        crate::mods::migrate_legacy_store(paths, &db)?;
        Ok(db)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("database connection lock poisoned")
    }
}

fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("failed to enable WAL mode")?;
    conn.pragma_update(None, "busy_timeout", 5000)
        .context("failed to set busy_timeout")?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .context("failed to enable foreign keys")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_paths(label: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        }
    }

    #[test]
    fn open_creates_db_file_and_is_idempotent() {
        let paths = temp_paths("open");
        {
            let _db = Db::open(&paths).unwrap();
        }
        assert!(paths.data_dir.join("odin.db").is_file());

        // Reopening (simulating a second process/invocation) must not error.
        let _db = Db::open(&paths).unwrap();
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn native_configuration_migration_is_resumable_and_preserves_file_values() {
        let paths = temp_paths("native-config-migration");
        let db_path = paths.data_dir.join("odin.db");
        {
            let mut conn = Connection::open(&db_path).unwrap();
            apply_pragmas(&conn).unwrap();
            migrations::run(&mut conn).unwrap();
            conn.execute_batch(
                "INSERT INTO game_instances (id, game, name, created_at) VALUES
                    ('rust-id', 'rust', 'rusty', '2026-01-01T00:00:00Z'),
                    ('pal-id', 'palworld', 'pals', '2026-01-01T00:00:00Z');
                 INSERT INTO rust_instance_configs
                    (instance_id, port, query_port, rcon_port, rcon_password, hostname, level, seed, world_size, max_players)
                    VALUES ('rust-id', 28015, 28016, 28017, 'secret', 'database name', 'Procedural Map', 42, 3000, 50);
                 INSERT INTO generic_game_instance_configs
                    (instance_id, port, query_port, admin_port, config_json)
                    VALUES ('pal-id', 8211, 27015, 8212, '{\"legacy\":true}');
                 INSERT INTO rust_file_config_migrations
                    VALUES ('rust-id', 'database name', 'Procedural Map', 42, 3000, 50);
                 INSERT INTO generic_config_json_migrations
                    VALUES ('pal-id', '{\"legacy\":true}');",
            )
            .unwrap();
        }
        let rust_cfg = paths
            .game_instance_dir(crate::game::GameId::Rust, "rusty")
            .join("server/cfg/server.cfg");
        std::fs::create_dir_all(rust_cfg.parent().unwrap()).unwrap();
        std::fs::write(&rust_cfg, "server.hostname \"operator name\"\n").unwrap();

        let db = Db::open(&paths).unwrap();

        let cfg = std::fs::read_to_string(&rust_cfg).unwrap();
        assert!(cfg.contains("server.hostname \"operator name\""));
        assert!(cfg.contains("server.level \"Procedural Map\""));
        assert_eq!(cfg.matches("server.hostname").count(), 1);
        let backup = paths
            .game_instance_dir(crate::game::GameId::Palworld, "pals")
            .join("legacy-config.json.disabled");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "{\"legacy\":true}"
        );
        assert_eq!(
            std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let conn = db.conn();
        let rust_columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('rust_instance_configs')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(!rust_columns.contains(&"hostname".to_string()));
        let generic_columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('generic_game_instance_configs')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(!generic_columns.contains(&"config_json".to_string()));
        let admin_port: Option<u16> = conn
            .query_row(
                "SELECT admin_port FROM generic_game_instance_configs WHERE instance_id = 'pal-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(admin_port, None);
        drop(conn);
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }

    #[test]
    fn failed_native_configuration_file_write_keeps_the_old_schema_for_retry() {
        let paths = temp_paths("native-config-retry");
        let db_path = paths.data_dir.join("odin.db");
        {
            let mut conn = Connection::open(&db_path).unwrap();
            apply_pragmas(&conn).unwrap();
            migrations::run(&mut conn).unwrap();
            conn.execute_batch(
                "INSERT INTO game_instances (id, game, name, created_at)
                    VALUES ('rust-id', 'rust', 'rusty', '2026-01-01T00:00:00Z');
                 INSERT INTO rust_instance_configs
                    (instance_id, port, query_port, rcon_port, rcon_password, hostname, level, seed, world_size, max_players)
                    VALUES ('rust-id', 28015, 28016, 28017, 'secret', 'Rusty', 'Procedural Map', 42, 3000, 50);
                 INSERT INTO rust_file_config_migrations
                    VALUES ('rust-id', 'Rusty', 'Procedural Map', 42, 3000, 50);",
            )
            .unwrap();
        }
        let obstruction = paths
            .game_instance_dir(crate::game::GameId::Rust, "rusty")
            .join("server");
        std::fs::create_dir_all(obstruction.parent().unwrap()).unwrap();
        std::fs::write(&obstruction, "not a directory").unwrap();

        assert!(Db::open(&paths).is_err());
        let conn = Connection::open(&db_path).unwrap();
        let hostname_still_present: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('rust_instance_configs') WHERE name = 'hostname')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let staging_still_present: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'rust_file_config_migrations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(hostname_still_present);
        assert!(staging_still_present);
        drop(conn);

        std::fs::remove_file(&obstruction).unwrap();
        let _db = Db::open(&paths).unwrap();
        assert!(obstruction.join("cfg/server.cfg").is_file());
        std::fs::remove_dir_all(&paths.data_dir).ok();
    }
}
