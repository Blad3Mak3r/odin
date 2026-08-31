//! Tracks every immutable `(mod_id, version)` payload in the shared store.

use anyhow::{Context, Result};
use rusqlite::params;

use super::Db;

pub fn contains(db: &Db, mod_id: &str, version: &str) -> Result<bool> {
    db.conn()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM global_mods WHERE mod_id = ?1 AND version = ?2)",
            params![mod_id, version],
            |row| row.get(0),
        )
        .with_context(|| format!("failed to look up '{mod_id}' version '{version}'"))
}

pub fn list_versions(db: &Db, mod_id: &str) -> Result<Vec<String>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT version FROM global_mods WHERE mod_id = ?1 ORDER BY updated_at DESC, version DESC",
    )?;
    stmt.query_map(params![mod_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .with_context(|| format!("failed to list stored versions for '{mod_id}'"))
}

pub fn list_all(db: &Db) -> Result<Vec<(String, String)>> {
    let conn = db.conn();
    let mut stmt =
        conn.prepare("SELECT mod_id, version FROM global_mods ORDER BY mod_id, updated_at DESC")?;
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to list global mod versions")
}

pub fn insert(db: &Db, mod_id: &str, version: &str) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO global_mods (mod_id, version, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(mod_id, version) DO UPDATE SET updated_at = excluded.updated_at",
            params![mod_id, version, chrono::Utc::now()],
        )
        .with_context(|| format!("failed to record '{mod_id}' version '{version}'"))?;
    Ok(())
}

pub fn remove(db: &Db, mod_id: &str) -> Result<()> {
    db.conn()
        .execute("DELETE FROM global_mods WHERE mod_id = ?1", params![mod_id])
        .with_context(|| format!("failed to remove global mod record for '{mod_id}'"))?;
    Ok(())
}

pub fn remove_version(db: &Db, mod_id: &str, version: &str) -> Result<()> {
    db.conn()
        .execute(
            "DELETE FROM global_mods WHERE mod_id = ?1 AND version = ?2",
            params![mod_id, version],
        )
        .with_context(|| format!("failed to remove '{mod_id}' version '{version}' record"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-global-mods-test-{label}-{}-{}",
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
    fn unknown_mod_has_no_version() {
        let db = temp_db("unknown");
        assert!(!contains(&db, "owner-mod", "1.0.0").unwrap());
    }

    #[test]
    fn set_then_get_round_trips() {
        let db = temp_db("roundtrip");
        insert(&db, "owner-mod", "1.0.0").unwrap();
        assert!(contains(&db, "owner-mod", "1.0.0").unwrap());
    }

    #[test]
    fn multiple_versions_are_retained() {
        let db = temp_db("multiple");
        insert(&db, "owner-mod", "1.0.0").unwrap();
        insert(&db, "owner-mod", "2.0.0").unwrap();
        let versions = list_versions(&db, "owner-mod").unwrap();
        assert_eq!(versions.len(), 2);
    }

    #[test]
    fn remove_clears_the_record() {
        let db = temp_db("remove");
        insert(&db, "owner-mod", "1.0.0").unwrap();
        remove(&db, "owner-mod").unwrap();
        assert!(!contains(&db, "owner-mod", "1.0.0").unwrap());
    }
}
