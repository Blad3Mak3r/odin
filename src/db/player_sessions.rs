use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::Serialize;

use super::Db;
use crate::game::GameId;

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
    joined_for_game(db, GameId::Valheim, instance, name, steam_id, at)
}

pub fn joined_for_game(
    db: &Db,
    game: GameId,
    instance: &str,
    name: &str,
    steam_id: Option<&str>,
    at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "INSERT INTO player_sessions (id, instance_id, name, steam_id, joined_at) \
         SELECT ?1, g.id, ?4, ?5, ?6 FROM game_instances g \
         WHERE g.game = ?2 AND g.name = ?3 AND NOT EXISTS (\
           SELECT 1 FROM player_sessions s WHERE s.instance_id = g.id \
             AND s.name = ?4 AND (?5 IS NULL OR s.steam_id = ?5) AND s.left_at IS NULL\
         )",
        params![
            uuid::Uuid::new_v4().to_string(),
            game.as_str(),
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
    left_for_game(db, GameId::Valheim, instance, name, steam_id, at)
}

pub fn left_for_game(
    db: &Db,
    game: GameId,
    instance: &str,
    name: &str,
    steam_id: Option<&str>,
    at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "UPDATE player_sessions SET left_at = ?5 WHERE id = (\
           SELECT s.id FROM player_sessions s JOIN game_instances g ON g.id = s.instance_id \
           WHERE g.game = ?1 AND g.name = ?2 AND s.name = ?3 \
             AND (?4 IS NULL OR s.steam_id = ?4) AND s.left_at IS NULL \
           ORDER BY s.joined_at DESC LIMIT 1\
         )",
        params![game.as_str(), instance, name, steam_id, at],
    )?;
    Ok(())
}

pub fn close_active(db: &Db, instance: &str, at: DateTime<Utc>) -> Result<()> {
    close_active_for_game(db, GameId::Valheim, instance, at)
}

pub fn close_active_for_game(
    db: &Db,
    game: GameId,
    instance: &str,
    at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "UPDATE player_sessions SET left_at = ?3 WHERE instance_id = (\
           SELECT id FROM game_instances WHERE game = ?1 AND name = ?2\
         ) AND left_at IS NULL",
        params![game.as_str(), instance, at],
    )?;
    Ok(())
}

/// Records a console-provided player snapshot. This is deliberately separate
/// from Valheim's event-driven registry: 7D2D only exposes this information
/// through a console command.
pub fn sync_for_game(
    db: &Db,
    game: GameId,
    instance: &str,
    players: &[(String, Option<String>)],
    at: DateTime<Utc>,
) -> Result<()> {
    let active = active_for_game(db, game, instance)?;
    for (name, steam_id) in players {
        joined_for_game(db, game, instance, name, steam_id.as_deref(), at)?;
    }
    for session in active {
        let still_present = players.iter().any(|(name, steam_id)| {
            name == &session.name
                && (steam_id.is_none()
                    || session.steam_id.is_none()
                    || steam_id.as_deref() == session.steam_id.as_deref())
        });
        if !still_present {
            left_for_game(
                db,
                game,
                instance,
                &session.name,
                session.steam_id.as_deref(),
                at,
            )?;
        }
    }
    Ok(())
}

pub fn recent(db: &Db, instance: &str, limit: usize) -> Result<Vec<PlayerSession>> {
    recent_for_game(db, GameId::Valheim, instance, limit)
}

pub fn recent_for_game(
    db: &Db,
    game: GameId,
    instance: &str,
    limit: usize,
) -> Result<Vec<PlayerSession>> {
    let conn = db.conn();
    let mut statement = conn.prepare(
        "SELECT s.id, s.name, s.steam_id, s.joined_at, s.left_at \
         FROM player_sessions s JOIN game_instances g ON g.id = s.instance_id \
         WHERE g.game = ?1 AND g.name = ?2 \
         ORDER BY s.joined_at DESC LIMIT ?3",
    )?;
    Ok(statement
        .query_map(params![game.as_str(), instance, limit], |row| {
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

fn active_for_game(db: &Db, game: GameId, instance: &str) -> Result<Vec<PlayerSession>> {
    let conn = db.conn();
    let mut statement = conn.prepare(
        "SELECT s.id, s.name, s.steam_id, s.joined_at, s.left_at \
         FROM player_sessions s JOIN game_instances g ON g.id = s.instance_id \
         WHERE g.game = ?1 AND g.name = ?2 AND s.left_at IS NULL",
    )?;
    Ok(statement
        .query_map(params![game.as_str(), instance], |row| {
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

    #[test]
    fn syncs_console_players_for_seven_days_without_mixing_valheim_history() {
        let dir = std::env::temp_dir().join(format!(
            "odin-seven-days-sessions-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let db = Db::open(&paths).unwrap();
        crate::db::game_instances::create_generic(&paths, &db, GameId::SevenDaysToDie, "navezgane")
            .unwrap();
        let joined_at = Utc::now();
        sync_for_game(
            &db,
            GameId::SevenDaysToDie,
            "navezgane",
            &[("Alice".into(), Some("EOS_123".into()))],
            joined_at,
        )
        .unwrap();
        let left_at = joined_at + chrono::Duration::minutes(5);
        sync_for_game(&db, GameId::SevenDaysToDie, "navezgane", &[], left_at).unwrap();

        let sessions = recent_for_game(&db, GameId::SevenDaysToDie, "navezgane", 10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "Alice");
        assert_eq!(sessions[0].left_at, Some(left_at));
        assert!(recent(&db, "navezgane", 10).unwrap().is_empty());

        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }
}
