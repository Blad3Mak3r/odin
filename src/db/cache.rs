//! A generic small-value cache, keyed by string — currently used only for
//! the fetched Thunderstore package index.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::params;

use super::Db;

pub struct CacheEntry {
    pub value: String,
    pub fetched_at: DateTime<Utc>,
}

pub fn get(db: &Db, key: &str) -> Result<Option<CacheEntry>> {
    let conn = db.conn();
    let mut stmt = conn.prepare("SELECT value, fetched_at FROM cache WHERE key = ?1")?;
    let mut rows = stmt.query(params![key])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    Ok(Some(CacheEntry {
        value: row.get(0)?,
        fetched_at: row.get(1)?,
    }))
}

pub fn set(db: &Db, key: &str, value: &str) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO cache (key, value, fetched_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, fetched_at = excluded.fetched_at",
            params![key, value, Utc::now()],
        )
        .with_context(|| format!("failed to cache '{key}'"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-cache-test-{label}-{}-{}",
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
    fn missing_key_is_none() {
        let db = temp_db("missing");
        assert!(get(&db, "nope").unwrap().is_none());
    }

    #[test]
    fn set_then_get_round_trips() {
        let db = temp_db("roundtrip");
        set(&db, "greeting", "hello").unwrap();
        let entry = get(&db, "greeting").unwrap().unwrap();
        assert_eq!(entry.value, "hello");
    }

    #[test]
    fn setting_twice_overwrites() {
        let db = temp_db("overwrite");
        set(&db, "greeting", "hello").unwrap();
        set(&db, "greeting", "goodbye").unwrap();
        let entry = get(&db, "greeting").unwrap().unwrap();
        assert_eq!(entry.value, "goodbye");
    }
}
