//! Tracks which version of each mod currently sits in the shared global mod
//! store (`<data_dir>/mods/<mod_id>`), replacing the old per-mod
//! `.odin-version` marker file.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use super::Db;

pub fn current_version(db: &Db, mod_id: &str) -> Result<Option<String>> {
    db.conn()
        .query_row(
            "SELECT version FROM global_mods WHERE mod_id = ?1",
            params![mod_id],
            |row| row.get(0),
        )
        .optional()
        .with_context(|| format!("failed to look up global mod version for '{mod_id}'"))
}

pub fn set_version(db: &Db, mod_id: &str, version: &str) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO global_mods (mod_id, version, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(mod_id) DO UPDATE SET version = excluded.version, updated_at = excluded.updated_at",
            params![mod_id, version, chrono::Utc::now()],
        )
        .with_context(|| format!("failed to record global mod version for '{mod_id}'"))?;
    Ok(())
}

/// Removes a mod's version record — called when its payload is pruned from
/// the shared store.
pub fn remove(db: &Db, mod_id: &str) -> Result<()> {
    db.conn()
        .execute("DELETE FROM global_mods WHERE mod_id = ?1", params![mod_id])
        .with_context(|| format!("failed to remove global mod record for '{mod_id}'"))?;
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
        assert!(current_version(&db, "owner-mod").unwrap().is_none());
    }

    #[test]
    fn set_then_get_round_trips() {
        let db = temp_db("roundtrip");
        set_version(&db, "owner-mod", "1.0.0").unwrap();
        assert_eq!(
            current_version(&db, "owner-mod").unwrap(),
            Some("1.0.0".to_string())
        );
    }

    #[test]
    fn setting_twice_overwrites() {
        let db = temp_db("overwrite");
        set_version(&db, "owner-mod", "1.0.0").unwrap();
        set_version(&db, "owner-mod", "2.0.0").unwrap();
        assert_eq!(
            current_version(&db, "owner-mod").unwrap(),
            Some("2.0.0".to_string())
        );
    }

    #[test]
    fn remove_clears_the_record() {
        let db = temp_db("remove");
        set_version(&db, "owner-mod", "1.0.0").unwrap();
        remove(&db, "owner-mod").unwrap();
        assert!(current_version(&db, "owner-mod").unwrap().is_none());
    }
}
