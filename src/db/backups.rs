//! Durable storage for backup metadata, backing `crate::backup`. The zip
//! files themselves stay on disk; only their id/timestamp/size live here.

use anyhow::{Context, Result};
use rusqlite::params;

use super::Db;
use crate::backup::BackupEntry;

/// Records one backup, replacing any existing row with the same id (an id is
/// a timestamp, so a collision would mean the same backup being re-recorded,
/// e.g. by the bootstrap importer running against an already-known entry).
pub fn insert(db: &Db, instance_name: &str, entry: &BackupEntry) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO backups (id, instance_name, created_at, size_bytes) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT(instance_name, id) DO UPDATE SET \
                created_at = excluded.created_at, \
                size_bytes = excluded.size_bytes",
            params![entry.id, instance_name, entry.created_at, entry.size_bytes as i64],
        )
        .with_context(|| format!("failed to record backup '{}' for '{instance_name}'", entry.id))?;
    Ok(())
}

/// Lists an instance's backups, newest first.
pub fn list(db: &Db, instance_name: &str) -> Result<Vec<BackupEntry>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, created_at, size_bytes FROM backups \
         WHERE instance_name = ?1 ORDER BY created_at DESC",
    )?;
    let entries = stmt
        .query_map(params![instance_name], |row| {
            Ok(BackupEntry {
                id: row.get(0)?,
                created_at: row.get(1)?,
                size_bytes: row.get::<_, i64>(2)? as u64,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-backups-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        crate::db::instances::save(
            &db,
            &crate::instance::state::InstanceState::new("my-server", 2456),
        )
        .unwrap();
        db
    }

    #[test]
    fn insert_then_list_round_trips_newest_first() {
        let db = temp_db("roundtrip");
        insert(
            &db,
            "my-server",
            &BackupEntry {
                id: "20260101T000000Z".to_string(),
                created_at: Utc::now(),
                size_bytes: 1024,
            },
        )
        .unwrap();
        insert(
            &db,
            "my-server",
            &BackupEntry {
                id: "20260102T000000Z".to_string(),
                created_at: Utc::now(),
                size_bytes: 2048,
            },
        )
        .unwrap();

        let entries = list(&db, "my-server").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "20260102T000000Z");
        assert_eq!(entries[1].id, "20260101T000000Z");
    }

    #[test]
    fn list_for_unknown_instance_is_empty() {
        let db = temp_db("empty");
        assert!(list(&db, "nope").unwrap().is_empty());
    }
}
