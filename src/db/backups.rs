//! Durable storage for backup metadata and local/remote object locations.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use super::Db;
use crate::backup::{BackupEntry, BackupRecord, BackupStorage};
use crate::backup_storage::{RemoteObject, StorageProvider};

/// Records one backup, replacing any existing row with the same id (an id is
/// a timestamp, so a collision would mean the same backup being re-recorded,
/// e.g. by the bootstrap importer running against an already-known entry).
pub fn insert(db: &Db, instance_name: &str, entry: &BackupEntry) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO backups (id, instance_name, instance_id, created_at, size_bytes) \
             SELECT ?1, ?2, id, ?3, ?4 FROM game_instances \
             WHERE game = 'valheim' AND name = ?2 \
             ON CONFLICT(instance_name, id) DO UPDATE SET \
                instance_id = excluded.instance_id, \
                created_at = excluded.created_at, \
                size_bytes = excluded.size_bytes, \
                remote_provider = NULL, \
                remote_endpoint = NULL, \
                remote_region = NULL, \
                remote_bucket = NULL, \
                remote_key = NULL",
            params![
                entry.id,
                instance_name,
                entry.created_at,
                entry.size_bytes as i64
            ],
        )
        .with_context(|| {
            format!(
                "failed to record backup '{}' for '{instance_name}'",
                entry.id
            )
        })?;
    Ok(())
}

/// Lists an instance's backups, newest first.
pub fn list(db: &Db, instance_name: &str) -> Result<Vec<BackupEntry>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT b.id, b.created_at, b.size_bytes, b.remote_provider, b.remote_endpoint, b.remote_region, \
                b.remote_bucket, b.remote_key FROM backups b \
         JOIN game_instances g ON g.id = b.instance_id \
         WHERE g.game = 'valheim' AND g.name = ?1 ORDER BY b.created_at DESC",
    )?;
    let rows = stmt
        .query_map(params![instance_name], stored_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(decode_record)
        .map(|record| record.map(|record| record.entry))
        .collect()
}

pub(crate) fn get(db: &Db, instance_name: &str, id: &str) -> Result<Option<BackupRecord>> {
    let row = db
        .conn()
        .query_row(
            "SELECT b.id, b.created_at, b.size_bytes, b.remote_provider, b.remote_endpoint, b.remote_region, \
                    b.remote_bucket, b.remote_key FROM backups b \
             JOIN game_instances g ON g.id = b.instance_id \
             WHERE g.game = 'valheim' AND g.name = ?1 AND b.id = ?2",
            params![instance_name, id],
            stored_row,
        )
        .optional()?;
    row.map(decode_record).transpose()
}

pub(crate) fn mark_remote(
    db: &Db,
    instance_name: &str,
    id: &str,
    object: &RemoteObject,
) -> Result<()> {
    db.conn()
        .execute(
            "UPDATE backups SET remote_provider = ?3, remote_endpoint = ?4, remote_region = ?5, \
                    remote_bucket = ?6, remote_key = ?7 \
             WHERE instance_id = (SELECT id FROM game_instances WHERE game = 'valheim' AND name = ?1) AND id = ?2",
            params![
                instance_name,
                id,
                object.provider.as_db(),
                object.endpoint,
                object.region,
                object.bucket,
                object.key,
            ],
        )
        .with_context(|| format!("failed to mark backup '{id}' as remote for '{instance_name}'"))?;
    Ok(())
}

/// Removes a backup's row. The caller is responsible for removing the zip
/// file itself.
pub fn delete(db: &Db, instance_name: &str, id: &str) -> Result<()> {
    db.conn()
        .execute(
            "DELETE FROM backups WHERE instance_id = (SELECT id FROM game_instances WHERE game = 'valheim' AND name = ?1) AND id = ?2",
            params![instance_name, id],
        )
        .with_context(|| format!("failed to delete backup '{id}' for '{instance_name}'"))?;
    Ok(())
}

type StoredRow = (
    String,
    chrono::DateTime<chrono::Utc>,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn stored_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn decode_record(row: StoredRow) -> Result<BackupRecord> {
    let (id, created_at, size_bytes, provider, endpoint, region, bucket, key) = row;
    let remote = match provider {
        None => None,
        Some(provider) => {
            let provider = StorageProvider::from_db(&provider)
                .with_context(|| format!("unknown storage provider on backup '{id}'"))?;
            Some(RemoteObject {
                provider,
                endpoint: endpoint.context("remote backup is missing its endpoint")?,
                region: region.context("remote backup is missing its region")?,
                bucket: bucket.context("remote backup is missing its bucket")?,
                key: key.context("remote backup is missing its object key")?,
            })
        }
    };
    let storage = remote
        .as_ref()
        .map_or(BackupStorage::Local, |object| object.provider.into());
    Ok(BackupRecord {
        entry: BackupEntry {
            id,
            created_at,
            size_bytes: size_bytes as u64,
            storage,
        },
        remote,
    })
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
                storage: BackupStorage::Local,
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
                storage: BackupStorage::Local,
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

    #[test]
    fn mark_remote_changes_the_listed_storage_and_preserves_the_object_location() {
        let db = temp_db("remote");
        let entry = BackupEntry {
            id: "20260101T000000Z".to_string(),
            created_at: Utc::now(),
            size_bytes: 1024,
            storage: BackupStorage::Local,
        };
        insert(&db, "my-server", &entry).unwrap();
        let object = RemoteObject {
            provider: StorageProvider::CloudflareR2,
            endpoint: "https://account.r2.cloudflarestorage.com".to_string(),
            region: "auto".to_string(),
            bucket: "odin-backups".to_string(),
            key: "odin/my-server/20260101T000000Z.zip".to_string(),
        };

        mark_remote(&db, "my-server", &entry.id, &object).unwrap();

        let stored = get(&db, "my-server", &entry.id).unwrap().unwrap();
        assert_eq!(
            (stored.entry.storage, stored.remote),
            (BackupStorage::CloudflareR2, Some(object))
        );
    }
}
