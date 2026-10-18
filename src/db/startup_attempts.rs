//! Durable history of container start attempts, written by
//! `crate::podman::startup` and read by the dashboard's Startup tab.
//!
//! Like `db::jobs`, this module only handles pre-encoded JSON strings so `db`
//! does not depend on the Podman layer's types.

use anyhow::{Context, Result};
use rusqlite::params;

use super::Db;

/// Attempts kept per instance; older ones are pruned when a new one begins.
pub const KEEP_PER_INSTANCE: i64 = 10;

/// One persisted attempt. `outcome` is `running`, `succeeded` or `failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRow {
    pub id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub outcome: String,
    pub events: String,
    pub failure: Option<String>,
}

/// Records a new running attempt and prunes the instance's older ones.
pub fn insert_running(db: &Db, instance_id: &str, started_at: &str) -> Result<i64> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO startup_attempts (instance_id, started_at, outcome) \
         VALUES (?1, ?2, 'running')",
        params![instance_id, started_at],
    )
    .context("failed to record startup attempt")?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM startup_attempts WHERE instance_id = ?1 AND id NOT IN \
         (SELECT id FROM startup_attempts WHERE instance_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![instance_id, KEEP_PER_INSTANCE],
    )
    .context("failed to prune startup attempts")?;
    Ok(id)
}

/// Stores the attempt's events as they accumulate, so a reader sees progress
/// while the start is still running.
pub fn update_events(db: &Db, id: i64, events: &str) -> Result<()> {
    db.conn()
        .execute(
            "UPDATE startup_attempts SET events = ?2 WHERE id = ?1",
            params![id, events],
        )
        .context("failed to update startup attempt")?;
    Ok(())
}

/// Closes an attempt as `succeeded` or `failed`.
pub fn finish(
    db: &Db,
    id: i64,
    outcome: &str,
    finished_at: &str,
    events: &str,
    failure: Option<&str>,
) -> Result<()> {
    db.conn()
        .execute(
            "UPDATE startup_attempts \
             SET outcome = ?2, finished_at = ?3, events = ?4, failure = ?5 WHERE id = ?1",
            params![id, outcome, finished_at, events, failure],
        )
        .context("failed to finish startup attempt")?;
    Ok(())
}

/// Drops an attempt that turned out not to be one (the instance was already
/// running).
pub fn delete(db: &Db, id: i64) -> Result<()> {
    db.conn()
        .execute("DELETE FROM startup_attempts WHERE id = ?1", params![id])
        .context("failed to delete startup attempt")?;
    Ok(())
}

/// The instance's attempts, newest first.
pub fn list(db: &Db, instance_id: &str) -> Result<Vec<AttemptRow>> {
    let conn = db.conn();
    let mut statement = conn
        .prepare(
            "SELECT id, started_at, finished_at, outcome, events, failure \
             FROM startup_attempts WHERE instance_id = ?1 ORDER BY id DESC",
        )
        .context("failed to prepare startup attempt query")?;
    let rows = statement
        .query_map(params![instance_id], |row| {
            Ok(AttemptRow {
                id: row.get(0)?,
                started_at: row.get(1)?,
                finished_at: row.get(2)?,
                outcome: row.get(3)?,
                events: row.get(4)?,
                failure: row.get(5)?,
            })
        })
        .context("failed to load startup attempts")?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to read startup attempts")?;
    Ok(rows)
}

/// Marks attempts still `running` as failed. Called once at startup: any such
/// row belongs to an Odin process that no longer exists.
pub fn fail_interrupted(db: &Db, finished_at: &str, failure: &str) -> Result<usize> {
    db.conn()
        .execute(
            "UPDATE startup_attempts SET outcome = 'failed', finished_at = ?1, failure = ?2 \
             WHERE outcome = 'running'",
            params![finished_at, failure],
        )
        .context("failed to close interrupted startup attempts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn db_with_instance() -> (Db, String, Paths) {
        let dir = std::env::temp_dir().join(format!("odin-startup-db-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        let crate::game::instances::GameInstance::Rust(instance) =
            crate::game::instances::create(&paths, &db, crate::game::GameId::Rust, "rusty")
                .unwrap()
        else {
            unreachable!("created a Rust instance")
        };
        (db, instance.identity.id, paths)
    }

    #[test]
    fn an_attempt_goes_from_running_to_failed_with_its_reason() {
        let (db, instance, paths) = db_with_instance();

        let id = insert_running(&db, &instance, "2026-10-18T10:00:00Z").unwrap();
        update_events(&db, id, r#"[{"phase":"image"}]"#).unwrap();
        let running = &list(&db, &instance).unwrap()[0];
        assert_eq!(running.outcome, "running");
        assert_eq!(running.events, r#"[{"phase":"image"}]"#);
        assert_eq!(running.finished_at, None);

        finish(
            &db,
            id,
            "failed",
            "2026-10-18T10:00:05Z",
            "[]",
            Some(r#"{"reason":"boom"}"#),
        )
        .unwrap();
        let failed = &list(&db, &instance).unwrap()[0];
        assert_eq!(failed.outcome, "failed");
        assert_eq!(failed.finished_at.as_deref(), Some("2026-10-18T10:00:05Z"));
        assert_eq!(failed.failure.as_deref(), Some(r#"{"reason":"boom"}"#));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn only_the_newest_attempts_are_kept_per_instance() {
        let (db, instance, paths) = db_with_instance();

        let ids: Vec<i64> = (0..KEEP_PER_INSTANCE + 3)
            .map(|_| insert_running(&db, &instance, "2026-10-18T10:00:00Z").unwrap())
            .collect();

        let kept: Vec<i64> = list(&db, &instance).unwrap().iter().map(|a| a.id).collect();
        let expected: Vec<i64> = ids
            .iter()
            .rev()
            .take(KEEP_PER_INSTANCE as usize)
            .copied()
            .collect();
        assert_eq!(kept, expected, "newest first, oldest pruned");
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn interrupted_attempts_are_failed_but_finished_ones_are_untouched() {
        let (db, instance, paths) = db_with_instance();
        let done = insert_running(&db, &instance, "t0").unwrap();
        finish(&db, done, "succeeded", "t1", "[]", None).unwrap();
        insert_running(&db, &instance, "t2").unwrap();

        let closed = fail_interrupted(&db, "t3", r#"{"reason":"restart"}"#).unwrap();

        assert_eq!(closed, 1);
        let rows = list(&db, &instance).unwrap();
        assert_eq!(rows[0].outcome, "failed");
        assert_eq!(rows[0].failure.as_deref(), Some(r#"{"reason":"restart"}"#));
        assert_eq!(rows[1].outcome, "succeeded");
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn deleting_an_instance_removes_its_attempts() {
        let (db, instance, paths) = db_with_instance();
        insert_running(&db, &instance, "t0").unwrap();

        db.conn()
            .execute(
                "DELETE FROM game_instances WHERE id = ?1",
                params![instance],
            )
            .unwrap();

        assert!(list(&db, &instance).unwrap().is_empty());
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn a_discarded_attempt_leaves_no_row() {
        let (db, instance, paths) = db_with_instance();
        let id = insert_running(&db, &instance, "t0").unwrap();

        delete(&db, id).unwrap();

        assert!(list(&db, &instance).unwrap().is_empty());
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
