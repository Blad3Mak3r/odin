//! Append-only history of security-relevant account events.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;

use super::Db;

/// Newest account and sign-in rows kept, whatever their age, so a flood of
/// failed sign-ins cannot grow the table without bound.
const MAX_SECURITY_ROWS: i64 = 10_000;
/// Newest rows kept for changes made through the API. They have their own
/// allowance so a busy fleet cannot push old sign-in failures out of the log.
const MAX_ACTION_ROWS: i64 = 5_000;
/// The `kind` of an entry recording a change made through the API.
pub const ACTION_KIND: &str = "action";

#[derive(Debug, Clone, Serialize)]
pub struct AuthEvent {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub kind: String,
    /// Who did it, or who tried: an email, or the login text for a failure.
    pub actor: Option<String>,
    pub target: Option<String>,
    pub ip: Option<String>,
    pub detail: Option<String>,
}

pub fn insert(
    db: &Db,
    kind: &str,
    actor: Option<&str>,
    target: Option<&str>,
    ip: Option<&str>,
    detail: Option<&str>,
) -> Result<()> {
    insert_capped(
        db,
        (kind, actor, target, ip, detail),
        (MAX_SECURITY_ROWS, MAX_ACTION_ROWS),
    )
}

type Entry<'a> = (
    &'a str,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

fn insert_capped(db: &Db, entry: Entry, (max_security, max_action): (i64, i64)) -> Result<()> {
    let (kind, actor, target, ip, detail) = entry;
    let conn = db.conn();
    conn.execute(
        "INSERT INTO auth_events (at, kind, actor, target, ip, detail) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![Utc::now(), kind, actor, target, ip, detail],
    )?;
    // Only the group the new row belongs to is trimmed.
    let (same_group, keep) = if kind == ACTION_KIND {
        ("kind = ?2", max_action)
    } else {
        ("kind <> ?2", max_security)
    };
    conn.execute(
        &format!(
            "DELETE FROM auth_events WHERE {same_group} AND id <= \
             (SELECT id FROM auth_events WHERE {same_group} \
              ORDER BY id DESC LIMIT 1 OFFSET ?1)"
        ),
        params![keep, ACTION_KIND],
    )?;
    Ok(())
}

/// The newest events first.
pub fn recent(db: &Db, limit: usize) -> Result<Vec<AuthEvent>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, at, kind, actor, target, ip, detail FROM auth_events \
         ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(AuthEvent {
            id: row.get(0)?,
            at: row.get(1)?,
            kind: row.get(2)?,
            actor: row.get(3)?,
            target: row.get(4)?,
            ip: row.get(5)?,
            detail: row.get(6)?,
        })
    })?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

pub fn purge_before(db: &Db, before: DateTime<Utc>) -> Result<usize> {
    Ok(db
        .conn()
        .execute("DELETE FROM auth_events WHERE at < ?1", params![before])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_db() -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-auth-events-test-{}-{}",
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

    fn add(db: &Db, kind: &str, caps: (i64, i64)) {
        insert_capped(db, (kind, None, None, None, None), caps).unwrap();
    }

    fn count(db: &Db, kind: &str) -> usize {
        recent(db, 100)
            .unwrap()
            .iter()
            .filter(|e| e.kind == kind)
            .count()
    }

    #[test]
    fn a_flood_of_actions_does_not_push_out_sign_in_events() {
        let db = temp_db();
        let caps = (3, 2);
        for _ in 0..3 {
            add(&db, "login_failed", caps);
        }
        for _ in 0..10 {
            add(&db, ACTION_KIND, caps);
        }

        assert_eq!(count(&db, "login_failed"), 3);
        assert_eq!(count(&db, ACTION_KIND), 2);
    }

    #[test]
    fn each_group_keeps_only_its_newest_rows() {
        let db = temp_db();
        let caps = (2, 2);
        for _ in 0..5 {
            add(&db, "login_succeeded", caps);
            add(&db, ACTION_KIND, caps);
        }

        assert_eq!(count(&db, "login_succeeded"), 2);
        assert_eq!(count(&db, ACTION_KIND), 2);
        let ids: Vec<_> = recent(&db, 100).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids.len(), 4);
        assert!(ids.windows(2).all(|w| w[0] > w[1]), "newest first");
    }
}
