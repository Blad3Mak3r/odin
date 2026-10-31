//! Failed-login counters that survive a restart.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;

use super::Db;

pub fn upsert(db: &Db, key: &str, failures: u32, at: DateTime<Utc>) -> Result<()> {
    db.conn().execute(
        "INSERT INTO auth_throttle (key, failures, last_failure_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(key) DO UPDATE SET failures = excluded.failures, \
         last_failure_at = excluded.last_failure_at",
        params![key, failures, at],
    )?;
    Ok(())
}

pub fn delete(db: &Db, key: &str) -> Result<()> {
    db.conn()
        .execute("DELETE FROM auth_throttle WHERE key = ?1", params![key])?;
    Ok(())
}

/// Counters whose last failure is newer than `since`.
pub fn load_since(db: &Db, since: DateTime<Utc>) -> Result<Vec<(String, u32, DateTime<Utc>)>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT key, failures, last_failure_at FROM auth_throttle WHERE last_failure_at > ?1",
    )?;
    let rows = stmt.query_map(params![since], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

pub fn purge_before(db: &Db, before: DateTime<Utc>) -> Result<usize> {
    Ok(db.conn().execute(
        "DELETE FROM auth_throttle WHERE last_failure_at < ?1",
        params![before],
    )?)
}
