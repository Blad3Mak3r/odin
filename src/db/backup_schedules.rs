//! Durable storage for per-instance backup schedules, backing
//! `web::backup_scheduler`'s background loop.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use super::Db;
use crate::game::GameId;

#[derive(Debug, Clone, PartialEq)]
pub struct BackupScheduleRow {
    pub game: GameId,
    pub instance_name: String,
    pub interval_hours: u32,
    pub retain_count: u32,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
}

/// Returns the schedule configured for an instance, if any — a missing row
/// just means scheduling was never turned on for it.
pub fn get_for_game(
    db: &Db,
    game: GameId,
    instance_name: &str,
) -> Result<Option<BackupScheduleRow>> {
    db.conn()
        .query_row(
            "SELECT instance_name, interval_hours, retain_count, enabled, last_run_at, ?1 \
             FROM backup_schedules WHERE instance_id = (SELECT id FROM game_instances WHERE game = ?1 AND name = ?2)",
            params![game.as_str(), instance_name],
            row_to_schedule,
        )
        .optional()
        .map_err(Into::into)
}

/// Creates or updates an instance's schedule, preserving `last_run_at` —
/// that's operational bookkeeping the scheduler loop owns, not something a
/// settings update should reset.
pub fn upsert_for_game(
    db: &Db,
    game: GameId,
    instance_name: &str,
    interval_hours: u32,
    retain_count: u32,
    enabled: bool,
) -> Result<()> {
    db.conn().execute(
        "INSERT INTO backup_schedules (instance_name, instance_id, interval_hours, retain_count, enabled) \
         SELECT ?2, id, ?3, ?4, ?5 FROM game_instances \
         WHERE game = ?1 AND name = ?2 \
         ON CONFLICT(instance_id) DO UPDATE SET \
             instance_id = excluded.instance_id, \
             interval_hours = excluded.interval_hours, \
             retain_count = excluded.retain_count, \
             enabled = excluded.enabled",
        params![game.as_str(), instance_name, interval_hours, retain_count, enabled],
    )?;
    Ok(())
}

/// Returns every enabled schedule that's due: never run before, or last run
/// at least `interval_hours` ago.
pub fn due(db: &Db, now: DateTime<Utc>) -> Result<Vec<BackupScheduleRow>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT g.name, s.interval_hours, s.retain_count, s.enabled, s.last_run_at, g.game \
         FROM backup_schedules s JOIN game_instances g ON g.id = s.instance_id \
         WHERE s.enabled = 1",
    )?;
    let rows = stmt.query_map([], row_to_schedule)?;
    let mut due = Vec::new();
    for row in rows {
        let schedule = row?;
        let is_due = match schedule.last_run_at {
            None => true,
            Some(last) => now - last >= chrono::Duration::hours(schedule.interval_hours as i64),
        };
        if is_due {
            due.push(schedule);
        }
    }
    Ok(due)
}

/// Records that a schedule just ran, so `due` doesn't re-trigger it until
/// another full interval has passed.
pub fn mark_run_for_game(
    db: &Db,
    game: GameId,
    instance_name: &str,
    at: DateTime<Utc>,
) -> Result<()> {
    db.conn().execute(
        "UPDATE backup_schedules SET last_run_at = ?3 \
         WHERE instance_id = (SELECT id FROM game_instances WHERE game = ?1 AND name = ?2)",
        params![game.as_str(), instance_name, at],
    )?;
    Ok(())
}

fn row_to_schedule(row: &rusqlite::Row) -> rusqlite::Result<BackupScheduleRow> {
    Ok(BackupScheduleRow {
        game: row
            .get::<_, String>(5)?
            .parse()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        instance_name: row.get(0)?,
        interval_hours: row.get(1)?,
        retain_count: row.get(2)?,
        enabled: row.get(3)?,
        last_run_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::state::InstanceState;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-backup-schedules-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        })
        .unwrap();
        crate::db::instances::save(&db, &InstanceState::new("my-server", 2456)).unwrap();
        db
    }

    #[test]
    fn same_named_games_keep_independent_schedules() {
        let db = temp_db("game-schedules");
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let paths = crate::paths::Paths {
            data_dir: root.clone(),
            config_dir: root.clone(),
        };
        crate::db::game_instances::create_rust(&paths, &db, "my-server").unwrap();
        upsert_for_game(&db, GameId::Valheim, "my-server", 12, 3, true).unwrap();
        upsert_for_game(&db, GameId::Rust, "my-server", 24, 7, true).unwrap();
        assert_eq!(
            get_for_game(&db, GameId::Valheim, "my-server")
                .unwrap()
                .unwrap()
                .retain_count,
            3
        );
        assert_eq!(
            get_for_game(&db, GameId::Rust, "my-server")
                .unwrap()
                .unwrap()
                .retain_count,
            7
        );
        assert_eq!(due(&db, Utc::now()).unwrap().len(), 2);
        crate::db::game_instances::delete_rust(&db, "my-server").unwrap();
        assert_eq!(due(&db, Utc::now()).unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_schedule_is_none() {
        let db = temp_db("missing");
        assert!(
            get_for_game(&db, GameId::Valheim, "my-server")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn upsert_then_get_round_trips() {
        let db = temp_db("roundtrip");
        upsert_for_game(&db, GameId::Valheim, "my-server", 24, 7, true).unwrap();

        let schedule = get_for_game(&db, GameId::Valheim, "my-server")
            .unwrap()
            .unwrap();
        assert_eq!(schedule.interval_hours, 24);
        assert_eq!(schedule.retain_count, 7);
        assert!(schedule.enabled);
        assert!(schedule.last_run_at.is_none());
    }

    #[test]
    fn upsert_twice_preserves_last_run_at() {
        let db = temp_db("preserve");
        upsert_for_game(&db, GameId::Valheim, "my-server", 24, 7, true).unwrap();
        let now = Utc::now();
        mark_run_for_game(&db, GameId::Valheim, "my-server", now).unwrap();

        upsert_for_game(&db, GameId::Valheim, "my-server", 12, 3, true).unwrap();

        let schedule = get_for_game(&db, GameId::Valheim, "my-server")
            .unwrap()
            .unwrap();
        assert_eq!(schedule.interval_hours, 12);
        assert_eq!(schedule.retain_count, 3);
        assert_eq!(schedule.last_run_at, Some(now));
    }

    #[test]
    fn due_only_returns_enabled_and_stale_schedules() {
        let db = temp_db("due");
        let now = Utc::now();

        upsert_for_game(&db, GameId::Valheim, "my-server", 24, 7, true).unwrap();
        assert_eq!(
            due(&db, now).unwrap().len(),
            1,
            "never run: due immediately"
        );

        mark_run_for_game(&db, GameId::Valheim, "my-server", now).unwrap();
        assert!(
            due(&db, now).unwrap().is_empty(),
            "just ran: not due again yet"
        );

        let later = now + chrono::Duration::hours(25);
        assert_eq!(
            due(&db, later).unwrap().len(),
            1,
            "interval elapsed: due again"
        );

        upsert_for_game(&db, GameId::Valheim, "my-server", 24, 7, false).unwrap();
        assert!(due(&db, later).unwrap().is_empty(), "disabled: never due");
    }
}
