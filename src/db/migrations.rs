//! Versioned schema migrations, tracked via SQLite's own `PRAGMA
//! user_version` rather than a migration framework — the schema is a
//! handful of tables and changes rarely, so a hand-rolled runner over a
//! few embedded `.sql` files is simpler than a new dependency.

use anyhow::{Context, Result};
use rusqlite::Connection;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "src/db/migrations"]
struct Migrations;

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
}
