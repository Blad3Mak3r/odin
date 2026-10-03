use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;

use super::Db;

#[derive(Debug, Clone, Serialize)]
pub struct PlayerSession {
    pub id: String,
    pub name: String,
    pub steam_id: Option<String>,
    pub joined_at: DateTime<Utc>,
    pub left_at: Option<DateTime<Utc>>,
}

pub fn joined(
    db: &Db,
    instance: &str,
    name: &str,
    steam_id: Option<&str>,
    at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "INSERT INTO player_sessions (id, instance_id, name, steam_id, joined_at) \
         SELECT ?1, g.id, ?3, ?4, ?5 FROM game_instances g \
         WHERE g.game = 'valheim' AND g.name = ?2 AND NOT EXISTS (\
           SELECT 1 FROM player_sessions s WHERE s.instance_id = g.id \
             AND s.name = ?3 AND (?4 IS NULL OR s.steam_id = ?4) AND s.left_at IS NULL\
         )",
        params![
            uuid::Uuid::new_v4().to_string(),
            instance,
            name,
            steam_id,
            at
        ],
    )?;
    Ok(())
}

pub fn left(
    db: &Db,
    instance: &str,
    name: &str,
    steam_id: Option<&str>,
    at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "UPDATE player_sessions SET left_at = ?4 WHERE id = (\
           SELECT s.id FROM player_sessions s JOIN game_instances g ON g.id = s.instance_id \
           WHERE g.game = 'valheim' AND g.name = ?1 AND s.name = ?2 \
             AND (?3 IS NULL OR s.steam_id = ?3) AND s.left_at IS NULL \
           ORDER BY s.joined_at DESC LIMIT 1\
         )",
        params![instance, name, steam_id, at],
    )?;
    Ok(())
}

pub fn close_active(db: &Db, instance: &str, at: DateTime<Utc>) -> Result<()> {
    db.conn().execute(
        "UPDATE player_sessions SET left_at = ?2 WHERE instance_id = (\
           SELECT id FROM game_instances WHERE game = 'valheim' AND name = ?1\
         ) AND left_at IS NULL",
        params![instance, at],
    )?;
    Ok(())
}

pub fn recent(db: &Db, instance: &str, limit: usize) -> Result<Vec<PlayerSession>> {
    let conn = db.conn();
    let mut statement = conn.prepare(
        "SELECT s.id, s.name, s.steam_id, s.joined_at, s.left_at \
         FROM player_sessions s JOIN game_instances g ON g.id = s.instance_id \
         WHERE g.game = 'valheim' AND g.name = ?1 \
         ORDER BY s.joined_at DESC LIMIT ?2",
    )?;
    Ok(statement
        .query_map(params![instance, limit], |row| {
            Ok(PlayerSession {
                id: row.get(0)?,
                name: row.get(1)?,
                steam_id: row.get(2)?,
                joined_at: row.get(3)?,
                left_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[test]
    fn records_and_closes_a_player_session() {
        let dir = std::env::temp_dir().join(format!(
            "odin-player-sessions-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let db = Db::open(&paths).unwrap();
        crate::instance::Instance::create(&paths, &db, "meadows").unwrap();
        let joined_at = Utc::now();
        let left_at = joined_at + chrono::Duration::minutes(12);

        joined(
            &db,
            "meadows",
            "Bjorn",
            Some("76561198000000000"),
            joined_at,
        )
        .unwrap();
        left(&db, "meadows", "Bjorn", Some("76561198000000000"), left_at).unwrap();

        let sessions = recent(&db, "meadows", 10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "Bjorn");
        assert_eq!(sessions[0].steam_id.as_deref(), Some("76561198000000000"));
        assert_eq!(sessions[0].joined_at, joined_at);
        assert_eq!(sessions[0].left_at, Some(left_at));

        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }
}
