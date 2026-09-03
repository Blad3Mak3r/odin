//! Durable storage for downsampled host/instance resource history, backing
//! `web::runtime::RuntimeRegistry`'s periodic persistence and the
//! longer-range history routes. The in-memory `RuntimeRegistry` buffer
//! stays the fast path for the live chart (~6 minutes at full 3s
//! resolution); this table holds a coarser, long-lived history (one sample
//! every few minutes, pruned past a retention window) for ranges beyond
//! that.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;

use super::Db;
use crate::game::GameId;

#[derive(Debug, Clone, Copy)]
pub struct ResourceSampleRow {
    pub at: DateTime<Utc>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

/// Records one downsampled sample. `instance_name: None` means a host-level
/// sample.
pub fn insert(
    db: &Db,
    instance_name: Option<&str>,
    at: DateTime<Utc>,
    cpu_percent: f32,
    memory_bytes: u64,
) -> Result<()> {
    match instance_name {
        Some(name) => {
            insert_for_instance(db, GameId::Valheim, name, at, cpu_percent, memory_bytes)?
        }
        None => {
            db.conn().execute(
                "INSERT INTO resource_samples (instance_name, at, cpu_percent, memory_bytes) \
                 VALUES (NULL, ?1, ?2, ?3)",
                params![at, cpu_percent, memory_bytes],
            )?;
        }
    }
    Ok(())
}

/// Records one downsampled sample for a specific game instance.
pub fn insert_for_instance(
    db: &Db,
    game: GameId,
    name: &str,
    at: DateTime<Utc>,
    cpu_percent: f32,
    memory_bytes: u64,
) -> Result<()> {
    db.conn().execute(
        "INSERT INTO resource_samples (instance_name, instance_id, at, cpu_percent, memory_bytes) \
         SELECT CASE WHEN ?1 = 'valheim' THEN ?2 END, id, ?3, ?4, ?5 \
         FROM game_instances WHERE game = ?1 AND name = ?2",
        params![game.as_str(), name, at, cpu_percent, memory_bytes],
    )?;
    Ok(())
}

/// Returns samples for one series (host, when `instance_name` is `None`, or
/// a specific instance) at or after `since`, oldest first.
pub fn range(
    db: &Db,
    instance_name: Option<&str>,
    since: DateTime<Utc>,
) -> Result<Vec<ResourceSampleRow>> {
    let conn = db.conn();
    let samples = match instance_name {
        Some(name) => {
            let mut stmt = conn.prepare(
                "SELECT at, cpu_percent, memory_bytes FROM resource_samples \
                 WHERE instance_id = (SELECT id FROM game_instances WHERE game = 'valheim' AND name = ?1) \
                 AND at >= ?2 ORDER BY at ASC",
            )?;
            stmt.query_map(params![name, since], row_to_sample)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT at, cpu_percent, memory_bytes FROM resource_samples \
                 WHERE instance_id IS NULL AND instance_name IS NULL AND at >= ?1 ORDER BY at ASC",
            )?;
            stmt.query_map(params![since], row_to_sample)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        }
    };
    Ok(samples)
}

fn row_to_sample(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResourceSampleRow> {
    Ok(ResourceSampleRow {
        at: row.get(0)?,
        cpu_percent: row.get(1)?,
        memory_bytes: row.get(2)?,
    })
}

/// Deletes every sample older than `before`, across every series.
pub fn prune_older_than(db: &Db, before: DateTime<Utc>) -> Result<()> {
    db.conn().execute(
        "DELETE FROM resource_samples WHERE at < ?1",
        params![before],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::state::InstanceState;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-resource-samples-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        })
        .unwrap()
    }

    #[test]
    fn host_and_instance_series_are_independent() {
        let db = temp_db("series");
        crate::db::instances::save(&db, &InstanceState::new("my-server", 2456)).unwrap();
        let now = Utc::now();

        insert(&db, None, now, 10.0, 1000).unwrap();
        insert(&db, Some("my-server"), now, 20.0, 2000).unwrap();

        let host = range(&db, None, now - chrono::Duration::seconds(1)).unwrap();
        assert_eq!(host.len(), 1);
        assert_eq!(host[0].cpu_percent, 10.0);

        let instance = range(&db, Some("my-server"), now - chrono::Duration::seconds(1)).unwrap();
        assert_eq!(instance.len(), 1);
        assert_eq!(instance[0].cpu_percent, 20.0);
    }

    #[test]
    fn range_excludes_samples_before_since() {
        let db = temp_db("range");
        let now = Utc::now();
        insert(&db, None, now - chrono::Duration::hours(2), 1.0, 100).unwrap();
        insert(&db, None, now, 2.0, 200).unwrap();

        let recent = range(&db, None, now - chrono::Duration::hours(1)).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].cpu_percent, 2.0);
    }

    #[test]
    fn game_instances_with_the_same_name_keep_separate_samples() {
        let db = temp_db("same-name");
        crate::db::instances::save(&db, &InstanceState::new("shared", 2456)).unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO game_instances (id, game, name, created_at) VALUES
                 ('rust-shared', 'rust', 'shared', '2024-01-01T00:00:00Z');",
            )
            .unwrap();
        let now = Utc::now();

        insert_for_instance(&db, GameId::Valheim, "shared", now, 10.0, 1000).unwrap();
        insert_for_instance(&db, GameId::Rust, "shared", now, 20.0, 2000).unwrap();

        let rust_cpu: f32 = db
            .conn()
            .query_row(
                "SELECT cpu_percent FROM resource_samples WHERE instance_id = 'rust-shared'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rust_cpu, 20.0);
    }

    #[test]
    fn rust_samples_do_not_require_a_valheim_instance() {
        let db = temp_db("rust-only");
        db.conn()
            .execute(
                "INSERT INTO game_instances (id, game, name, created_at) VALUES
                 ('rust-only', 'rust', 'rust-only', '2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();

        let result = insert_for_instance(&db, GameId::Rust, "rust-only", Utc::now(), 1.0, 1);

        assert!(result.is_ok());
    }

    #[test]
    fn prune_older_than_removes_stale_rows_only() {
        let db = temp_db("prune");
        let now = Utc::now();
        insert(&db, None, now - chrono::Duration::days(10), 1.0, 100).unwrap();
        insert(&db, None, now, 2.0, 200).unwrap();

        prune_older_than(&db, now - chrono::Duration::days(7)).unwrap();

        let remaining = range(&db, None, now - chrono::Duration::days(30)).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].cpu_percent, 2.0);
    }
}
