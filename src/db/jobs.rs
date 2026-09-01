//! Durable storage for job history, backing `crate::web::jobs::JobRegistry`.
//!
//! Deliberately decoupled from the web layer's job types: this module only
//! ever handles pre-encoded JSON strings, the same way `db::activity` treats
//! an event's payload, so `db` doesn't need to depend on `web`.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;

use super::Db;

/// One job's persisted row, decoded just enough for the caller (the web
/// layer, which owns the concrete `JobKindDescr`/`JobStatus` types) to
/// finish reconstructing it.
pub struct JobRow {
    pub id: String,
    pub kind_payload: String,
    pub status_payload: String,
    pub started_at: DateTime<Utc>,
    pub log: Vec<String>,
}

/// Inserts a newly spawned job's initial row.
#[allow(clippy::too_many_arguments)]
pub fn insert(
    db: &Db,
    id: &str,
    kind_tag: &str,
    kind_payload: &str,
    status_tag: &str,
    status_payload: &str,
    started_at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "INSERT INTO jobs (id, kind, kind_payload, status, status_payload, started_at, log) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, '[]')",
        params![
            id,
            kind_tag,
            kind_payload,
            status_tag,
            status_payload,
            started_at
        ],
    )?;
    Ok(())
}

/// Updates a job's status in place. A no-op if the id doesn't exist.
pub fn update_status(db: &Db, id: &str, status_tag: &str, status_payload: &str) -> Result<()> {
    db.conn().execute(
        "UPDATE jobs SET status = ?2, status_payload = ?3 WHERE id = ?1",
        params![id, status_tag, status_payload],
    )?;
    Ok(())
}

/// Replaces a job's log wholesale — simpler than a separate line-per-row
/// table, and cheap enough given the log is capped at a few thousand short
/// lines.
pub fn set_log(db: &Db, id: &str, log: &[String]) -> Result<()> {
    let payload = serde_json::to_string(log)?;
    db.conn().execute(
        "UPDATE jobs SET log = ?2 WHERE id = ?1",
        params![id, payload],
    )?;
    Ok(())
}

/// Returns up to `limit` most recent jobs, most recent first.
pub fn recent(db: &Db, limit: usize) -> Result<Vec<JobRow>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, kind_payload, status_payload, started_at, log \
         FROM jobs ORDER BY started_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        let log_json: String = row.get(4)?;
        Ok(JobRow {
            id: row.get(0)?,
            kind_payload: row.get(1)?,
            status_payload: row.get(2)?,
            started_at: row.get(3)?,
            log: serde_json::from_str(&log_json).unwrap_or_default(),
        })
    })?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

/// Deletes terminal jobs older than `before`, preserving queued and running jobs.
/// Returns the IDs actually deleted so the in-memory registry can stay in sync.
pub fn delete_finished_before(db: &Db, before: DateTime<Utc>) -> Result<Vec<String>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "DELETE FROM jobs \
         WHERE started_at < ?1 AND status IN ('succeeded', 'failed') \
         RETURNING id",
    )?;
    let rows = stmt.query_map(params![before], |row| row.get(0))?;
    rows.map(|row| row.map_err(Into::into)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-jobs-test-{label}-{}-{}",
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
    fn insert_then_recent_round_trips() {
        let db = temp_db("roundtrip");
        insert(
            &db,
            "job-1",
            "steamcmd_install",
            r#"{"kind":"steamcmd_install"}"#,
            "queued",
            r#"{"status":"queued"}"#,
            Utc::now(),
        )
        .unwrap();

        let rows = recent(&db, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "job-1");
        assert_eq!(rows[0].status_payload, r#"{"status":"queued"}"#);
        assert!(rows[0].log.is_empty());
    }

    #[test]
    fn update_status_and_set_log_persist() {
        let db = temp_db("update");
        insert(
            &db,
            "job-1",
            "steamcmd_install",
            r#"{"kind":"steamcmd_install"}"#,
            "queued",
            r#"{"status":"queued"}"#,
            Utc::now(),
        )
        .unwrap();

        update_status(&db, "job-1", "succeeded", r#"{"status":"succeeded"}"#).unwrap();
        set_log(
            &db,
            "job-1",
            &["line one".to_string(), "line two".to_string()],
        )
        .unwrap();

        let rows = recent(&db, 10).unwrap();
        assert_eq!(rows[0].status_payload, r#"{"status":"succeeded"}"#);
        assert_eq!(
            rows[0].log,
            vec!["line one".to_string(), "line two".to_string()]
        );
    }

    #[test]
    fn recent_orders_most_recent_first_and_respects_limit() {
        let db = temp_db("order");
        for i in 0..5 {
            insert(
                &db,
                &format!("job-{i}"),
                "steamcmd_install",
                r#"{"kind":"steamcmd_install"}"#,
                "queued",
                r#"{"status":"queued"}"#,
                Utc::now() + chrono::Duration::seconds(i),
            )
            .unwrap();
        }

        let rows = recent(&db, 3).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, "job-4");
        assert_eq!(rows[1].id, "job-3");
        assert_eq!(rows[2].id, "job-2");
    }

    #[test]
    fn delete_finished_before_preserves_active_and_recent_jobs() {
        let db = temp_db("retention");
        let cutoff = Utc::now();
        let jobs = [
            (
                "old-succeeded",
                "succeeded",
                r#"{"status":"succeeded"}"#,
                cutoff - chrono::Duration::seconds(1),
            ),
            (
                "old-failed",
                "failed",
                r#"{"status":"failed","message":"boom"}"#,
                cutoff - chrono::Duration::seconds(1),
            ),
            (
                "old-queued",
                "queued",
                r#"{"status":"queued"}"#,
                cutoff - chrono::Duration::seconds(1),
            ),
            (
                "old-running",
                "running",
                r#"{"status":"running"}"#,
                cutoff - chrono::Duration::seconds(1),
            ),
            (
                "at-cutoff",
                "succeeded",
                r#"{"status":"succeeded"}"#,
                cutoff,
            ),
        ];
        for (id, status, payload, started_at) in jobs {
            insert(
                &db,
                id,
                "steamcmd_install",
                r#"{"kind":"steamcmd_install"}"#,
                status,
                payload,
                started_at,
            )
            .unwrap();
        }

        assert_eq!(delete_finished_before(&db, cutoff).unwrap().len(), 2);
        let ids = recent(&db, 10)
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"old-queued".to_string()));
        assert!(ids.contains(&"old-running".to_string()));
        assert!(ids.contains(&"at-cutoff".to_string()));
    }
}
