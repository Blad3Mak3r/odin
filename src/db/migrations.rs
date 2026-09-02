//! Versioned schema migrations, tracked via SQLite's own `PRAGMA
//! user_version` rather than a migration framework — the schema is a
//! handful of tables and changes rarely, so a hand-rolled runner over a
//! few embedded `.sql` files is simpler than a new dependency.

use anyhow::{Context, Result};
use std::path::Path;

use rusqlite::{Connection, params};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "src/db/migrations"]
struct Migrations;

/// Takes one consistent SQLite snapshot immediately before the first
/// multi-game migration. `VACUUM INTO` includes WAL content, unlike copying
/// the main database file while another Odin process is active.
pub fn backup_before_game_instances(conn: &Connection, db_path: &Path) -> Result<()> {
    const GAME_INSTANCES_VERSION: i64 = 12;
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current == 0 || current >= GAME_INSTANCES_VERSION {
        return Ok(());
    }

    let backup_path = db_path.with_file_name("odin.pre-game-instances.db");
    if backup_path.exists() {
        return Ok(());
    }
    conn.execute(
        "VACUUM INTO ?1",
        params![backup_path.to_string_lossy().as_ref()],
    )
    .context("failed to create pre-multi-game database backup")?;
    Ok(())
}

/// Applies every embedded migration newer than the DB's current
/// `user_version`, in filename order, each inside its own transaction.
/// Safe to call on every `Db::open` — a fully migrated DB is a no-op.
pub fn run(conn: &mut Connection) -> Result<()> {
    let current_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    let mut files: Vec<String> = Migrations::iter().map(|f| f.to_string()).collect();
    files.sort();

    for file in files {
        let version = migration_version(&file).with_context(|| {
            format!("migration file '{file}' doesn't start with a version number")
        })?;
        if version <= current_version {
            continue;
        }

        let sql = Migrations::get(&file)
            .with_context(|| format!("failed to load embedded migration '{file}'"))?;
        let sql = std::str::from_utf8(&sql.data)
            .with_context(|| format!("migration '{file}' is not valid UTF-8"))?;

        let tx = conn.transaction()?;
        tx.execute_batch(sql)
            .with_context(|| format!("failed to apply migration '{file}'"))?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(migration = file, "applied database migration");
    }

    Ok(())
}

/// Extracts the leading integer from a filename like `0001_init.sql`.
fn migration_version(file: &str) -> Option<i64> {
    file.split('_').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_ends_at_latest_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert!(version > 0);

        // Schema actually applied: a core table should now exist.
        let table_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'instances')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(table_exists);
    }

    #[test]
    fn v1_tmux_session_column_is_dropped_and_pid_columns_are_nullable() {
        let mut conn = Connection::open_in_memory().unwrap();
        // Apply only 0001 by hand, seed a v1-shaped row, then run the full
        // migration set and check 0002 both drops `tmux_session` and adds
        // nullable `pid`/`pid_started_at` without losing the row.
        conn.execute_batch(
            std::str::from_utf8(&Migrations::get("0001_init.sql").unwrap().data).unwrap(),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO instances (name, port, world_name, public, created_at, tmux_session) \
             VALUES ('legacy', 2456, 'legacy', 1, '2024-01-01T00:00:00Z', 'odin-legacy')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();

        run(&mut conn).unwrap();

        let has_tmux_session: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('instances') WHERE name = 'tmux_session')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!has_tmux_session);

        let (pid, name): (Option<i64>, String) = conn
            .query_row(
                "SELECT pid, name FROM instances WHERE name = 'legacy'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(pid, None);
        assert_eq!(name, "legacy");
    }

    #[test]
    fn running_twice_is_a_noop() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        let first: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();

        run(&mut conn).unwrap();
        let second: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn v11_preserves_legacy_bepinex_as_installed_with_unknown_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        let mut files: Vec<String> = Migrations::iter().map(|file| file.to_string()).collect();
        files.sort();
        for file in files {
            let version = migration_version(&file).unwrap();
            if version >= 11 {
                continue;
            }
            let migration = Migrations::get(&file).unwrap();
            conn.execute_batch(std::str::from_utf8(&migration.data).unwrap())
                .unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        conn.execute(
            "INSERT INTO instances (name, port, world_name, public, created_at, bepinex_installed) VALUES ('legacy', 2456, 'legacy', 1, '2024-01-01T00:00:00Z', 1)",
            [],
        ).unwrap();
        run(&mut conn).unwrap();
        let state: (bool, Option<String>) = conn
            .query_row(
                "SELECT bepinex_installed, bepinex_version FROM instances WHERE name = 'legacy'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, (true, None));
    }

    #[test]
    fn valheim_config_moves_under_the_generic_identity() {
        let mut conn = Connection::open_in_memory().unwrap();
        let mut files: Vec<String> = Migrations::iter().map(|file| file.to_string()).collect();
        files.sort();
        for file in files {
            let version = migration_version(&file).unwrap();
            if version >= 12 {
                continue;
            }
            let migration = Migrations::get(&file).unwrap();
            conn.execute_batch(std::str::from_utf8(&migration.data).unwrap())
                .unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        conn.execute(
            "INSERT INTO instances (name, port, world_name, public, created_at) VALUES ('legacy', 2456, 'legacy-world', 1, '2024-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        run(&mut conn).unwrap();

        let config: (String, u16, String) = conn
            .query_row(
                "SELECT g.id, v.port, v.world_name FROM game_instances g JOIN valheim_instance_configs v ON v.instance_id = g.id WHERE g.game = 'valheim' AND g.name = 'legacy'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert!(!config.0.is_empty());
        assert_eq!(config.1, 2456);
        assert_eq!(config.2, "legacy-world");
    }

    #[test]
    fn valheim_references_gain_the_generic_identity() {
        let mut conn = Connection::open_in_memory().unwrap();
        let mut files: Vec<String> = Migrations::iter().map(|file| file.to_string()).collect();
        files.sort();
        for file in files {
            let version = migration_version(&file).unwrap();
            if version >= 12 {
                continue;
            }
            let migration = Migrations::get(&file).unwrap();
            conn.execute_batch(std::str::from_utf8(&migration.data).unwrap())
                .unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        conn.execute_batch(
            "INSERT INTO instances (name, port, world_name, public, created_at) VALUES ('legacy', 2456, 'legacy', 1, '2024-01-01T00:00:00Z');
             INSERT INTO installed_mods (instance_name, mod_id, version, installed_at, enabled) VALUES ('legacy', 'owner-mod', '1.0.0', '2024-01-01T00:00:00Z', 1);
             INSERT INTO access_list_entries (instance_name, kind, steam_id) VALUES ('legacy', 'admin', '76561197960287930');
             INSERT INTO backups (id, instance_name, created_at, size_bytes) VALUES ('backup', 'legacy', '2024-01-01T00:00:00Z', 1);
             INSERT INTO backup_schedules (instance_name, interval_hours, retain_count, enabled) VALUES ('legacy', 24, 7, 1);
             INSERT INTO backup_storage_configs (instance_name, provider, endpoint, region, bucket, access_key_id, secret_access_key) VALUES ('legacy', 'aws_s3', 'endpoint', 'region', 'bucket', 'key', 'secret');
             INSERT INTO resource_samples (instance_name, at, cpu_percent, memory_bytes) VALUES ('legacy', '2024-01-01T00:00:00Z', 1, 1);
             INSERT INTO activity_events (id, at, instance, kind) VALUES ('event', '2024-01-01T00:00:00Z', 'legacy', 'instance_started');",
        )
        .unwrap();

        run(&mut conn).unwrap();

        let missing: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM (
                    SELECT instance_id FROM installed_mods
                    UNION ALL SELECT instance_id FROM access_list_entries
                    UNION ALL SELECT instance_id FROM backups
                    UNION ALL SELECT instance_id FROM backup_schedules
                    UNION ALL SELECT instance_id FROM backup_storage_configs
                    UNION ALL SELECT instance_id FROM resource_samples
                    UNION ALL SELECT instance_id FROM activity_events
                 ) WHERE instance_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(missing, 0);
    }

    #[test]
    fn v10_preserves_the_payload_version_instances_actually_used() {
        let mut conn = Connection::open_in_memory().unwrap();
        let mut files: Vec<String> = Migrations::iter().map(|file| file.to_string()).collect();
        files.sort();
        for file in files {
            let version = migration_version(&file).unwrap();
            if version >= 10 {
                continue;
            }
            let migration = Migrations::get(&file).unwrap();
            conn.execute_batch(std::str::from_utf8(&migration.data).unwrap())
                .unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        conn.execute(
            "INSERT INTO instances (name, port, world_name, public, created_at) \
             VALUES ('legacy', 2456, 'legacy', 1, '2024-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO global_mods (mod_id, version, updated_at) \
             VALUES ('owner-mod', '2.0.0', '2024-01-02T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO installed_mods (instance_name, mod_id, version, installed_at, enabled) \
             VALUES ('legacy', 'owner-mod', '1.0.0', '2024-01-01T00:00:00Z', 1)",
            [],
        )
        .unwrap();

        run(&mut conn).unwrap();

        let migrated: (String, bool) = conn
            .query_row(
                "SELECT version, pinned FROM installed_mods WHERE instance_name = 'legacy'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(migrated, ("2.0.0".to_string(), false));
        conn.execute(
            "INSERT INTO global_mods (mod_id, version, updated_at) \
             VALUES ('owner-mod', '3.0.0', '2024-01-03T00:00:00Z')",
            [],
        )
        .unwrap();
    }
}
