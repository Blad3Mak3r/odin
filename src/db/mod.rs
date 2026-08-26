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
//! `mods`, `lists`, `backups`, `activity`, `cache`) that takes `&Db` and
//! exposes calls shaped like the file-I/O functions they replace.

pub mod activity;
pub mod backups;
pub mod cache;
pub mod global_mods;
mod import;
pub mod instances;
mod migrations;

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
        migrations::run(&mut conn)?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        import::bootstrap_if_empty(&db, paths)?;
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
}
