//! Durable storage for access lists, backing `crate::instance::lists`. The
//! on-disk `.txt` files Valheim reads directly are regenerated from these
//! rows by the caller — this module only knows about the database side.

use anyhow::{Context, Result};
use rusqlite::params;

use super::Db;

pub fn read(db: &Db, instance_name: &str, kind: &str) -> Result<Vec<String>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT l.steam_id FROM access_list_entries l \
         JOIN game_instances g ON g.id = l.instance_id \
         WHERE g.game = 'valheim' AND g.name = ?1 AND l.kind = ?2 ORDER BY l.steam_id",
    )?;
    let ids = stmt
        .query_map(params![instance_name, kind], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

/// Replaces a list's contents wholesale, in one transaction.
pub fn replace(db: &Db, instance_name: &str, kind: &str, ids: &[String]) -> Result<()> {
    let mut conn = db.conn();
    let tx = conn
        .transaction()
        .context("failed to start list update transaction")?;
    tx.execute(
        "DELETE FROM access_list_entries WHERE instance_name = ?1 AND kind = ?2",
        params![instance_name, kind],
    )?;
    for id in ids {
        tx.execute(
            "INSERT INTO access_list_entries (instance_name, instance_id, kind, steam_id) \
             SELECT ?1, id, ?2, ?3 FROM game_instances \
             WHERE game = 'valheim' AND name = ?1",
            params![instance_name, kind, id],
        )?;
    }
    tx.commit().context("failed to commit list update")?;
    Ok(())
}

/// Inserts one row directly, without clearing existing ones — used only by
/// the bootstrap importer, which seeds several instances' lists in bulk.
pub(super) fn insert(db: &Db, instance_name: &str, kind: &str, id: &str) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO access_list_entries (instance_name, instance_id, kind, steam_id) \
             SELECT ?1, id, ?2, ?3 FROM game_instances \
             WHERE game = 'valheim' AND name = ?1",
            params![instance_name, kind, id],
        )
        .with_context(|| format!("failed to import list entry for '{instance_name}'"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::state::InstanceState;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-lists-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        crate::db::instances::save(&db, &InstanceState::new("my-server", 2456)).unwrap();
        db
    }

    #[test]
    fn replace_then_read_round_trips() {
        let db = temp_db("roundtrip");
        let ids = vec![
            "76561197960287930".to_string(),
            "76561197960287931".to_string(),
        ];
        replace(&db, "my-server", "admin", &ids).unwrap();
        assert_eq!(read(&db, "my-server", "admin").unwrap(), ids);
    }

    #[test]
    fn replacing_clears_previous_entries() {
        let db = temp_db("replace");
        replace(
            &db,
            "my-server",
            "admin",
            &["76561197960287930".to_string()],
        )
        .unwrap();
        replace(
            &db,
            "my-server",
            "admin",
            &["76561197960287931".to_string()],
        )
        .unwrap();
        assert_eq!(
            read(&db, "my-server", "admin").unwrap(),
            vec!["76561197960287931".to_string()]
        );
    }

    #[test]
    fn different_kinds_are_independent() {
        let db = temp_db("kinds");
        replace(
            &db,
            "my-server",
            "admin",
            &["76561197960287930".to_string()],
        )
        .unwrap();
        assert!(read(&db, "my-server", "banned").unwrap().is_empty());
    }
}
