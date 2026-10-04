//! Durable daily operating windows for game instances.

use anyhow::{Context, Result};
use chrono::{Local, Timelike};
use rusqlite::{OptionalExtension, params};

use super::Db;
use crate::game::GameId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UptimeSchedule {
    pub enabled: bool,
    /// Minutes after midnight in the host's local time zone.
    pub start_minute: u16,
    /// Minutes after midnight in the host's local time zone.
    pub stop_minute: u16,
}

impl UptimeSchedule {
    /// Whether this daily window includes a minute after local midnight.
    /// The stop minute is exclusive, and a stop time before the start time
    /// represents an overnight window.
    pub fn includes(&self, minute: u16) -> bool {
        if self.start_minute < self.stop_minute {
            (self.start_minute..self.stop_minute).contains(&minute)
        } else {
            minute >= self.start_minute || minute < self.stop_minute
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledInstance {
    pub game: GameId,
    pub name: String,
    pub schedule: UptimeSchedule,
}

pub fn get_for_game(db: &Db, game: GameId, name: &str) -> Result<Option<UptimeSchedule>> {
    db.conn()
        .query_row(
            "SELECT s.enabled, s.start_minute, s.stop_minute \
             FROM uptime_schedules s JOIN game_instances g ON g.id = s.instance_id \
             WHERE g.game = ?1 AND g.name = ?2",
            params![game.as_str(), name],
            row_to_schedule,
        )
        .optional()
        .map_err(Into::into)
}

pub fn upsert_for_game(db: &Db, game: GameId, name: &str, schedule: &UptimeSchedule) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO uptime_schedules (instance_id, enabled, start_minute, stop_minute) \
             SELECT id, ?3, ?4, ?5 FROM game_instances WHERE game = ?1 AND name = ?2 \
             ON CONFLICT(instance_id) DO UPDATE SET \
                 enabled = excluded.enabled, start_minute = excluded.start_minute, stop_minute = excluded.stop_minute",
            params![game.as_str(), name, schedule.enabled, schedule.start_minute, schedule.stop_minute],
        )
        .context("failed to save uptime schedule")?;
    Ok(())
}

pub fn enabled(db: &Db) -> Result<Vec<ScheduledInstance>> {
    let conn = db.conn();
    let mut statement = conn.prepare(
        "SELECT g.game, g.name, s.enabled, s.start_minute, s.stop_minute \
         FROM uptime_schedules s JOIN game_instances g ON g.id = s.instance_id \
         WHERE s.enabled = 1",
    )?;
    statement
        .query_map([], |row| {
            Ok(ScheduledInstance {
                game: row
                    .get::<_, String>(0)?
                    .parse()
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                name: row.get(1)?,
                schedule: UptimeSchedule {
                    enabled: row.get(2)?,
                    start_minute: row.get(3)?,
                    stop_minute: row.get(4)?,
                },
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// A missing or disabled schedule deliberately imposes no restriction. This
/// lets crash recovery retain its existing behavior until an administrator
/// explicitly enables operating hours.
pub fn permits_running_now(db: &Db, game: GameId, name: &str) -> Result<bool> {
    let now = Local::now();
    let minute = (now.hour() * 60 + now.minute()) as u16;
    Ok(get_for_game(db, game, name)?
        .is_none_or(|schedule| !schedule.enabled || schedule.includes(minute)))
}

fn row_to_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<UptimeSchedule> {
    Ok(UptimeSchedule {
        enabled: row.get(0)?,
        start_minute: row.get(1)?,
        stop_minute: row.get(2)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn context(label: &str) -> (Paths, Db) {
        let root = std::env::temp_dir().join(format!(
            "odin-uptime-schedule-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = Paths {
            data_dir: root.clone(),
            config_dir: root,
        };
        let db = Db::open(&paths).unwrap();
        (paths, db)
    }

    #[test]
    fn keeps_same_named_game_schedules_independent() {
        let (paths, db) = context("independent");
        crate::instance::Instance::create(&paths, &db, "shared").unwrap();
        crate::db::game_instances::create_rust(&paths, &db, "shared").unwrap();
        let valheim = UptimeSchedule {
            enabled: true,
            start_minute: 480,
            stop_minute: 1320,
        };
        let rust = UptimeSchedule {
            enabled: true,
            start_minute: 600,
            stop_minute: 1200,
        };
        upsert_for_game(&db, GameId::Valheim, "shared", &valheim).unwrap();
        upsert_for_game(&db, GameId::Rust, "shared", &rust).unwrap();

        assert_eq!(
            get_for_game(&db, GameId::Valheim, "shared").unwrap(),
            Some(valheim)
        );
        assert_eq!(
            get_for_game(&db, GameId::Rust, "shared").unwrap(),
            Some(rust)
        );
        assert_eq!(enabled(&db).unwrap().len(), 2);
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
