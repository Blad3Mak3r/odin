//! Append-only history of security-relevant account events.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;

use super::Db;

/// Newest rows kept, whatever their age, so a flood of failed sign-ins
/// cannot grow the table without bound.
const MAX_ROWS: i64 = 10_000;

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
    let conn = db.conn();
    conn.execute(
        "INSERT INTO auth_events (at, kind, actor, target, ip, detail) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![Utc::now(), kind, actor, target, ip, detail],
    )?;
    conn.execute(
        "DELETE FROM auth_events WHERE id <= \
         (SELECT id FROM auth_events ORDER BY id DESC LIMIT 1 OFFSET ?1)",
        params![MAX_ROWS],
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
