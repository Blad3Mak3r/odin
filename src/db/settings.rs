//! Durable, user-entered global configuration — kept in its own table
//! rather than folded into `cache`:
//! `cache` carries TTL/refresh semantics for re-fetchable data, while a
//! setting is durable input that should never be silently evicted.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use super::Db;

pub const NEXUS_API_KEY: &str = "nexus_api_key";
const DEFAULT_AUTO_RESTART: &str = "default_auto_restart";
const DEFAULT_BACKUP_ENABLED: &str = "default_backup_enabled";
const DEFAULT_BACKUP_INTERVAL_HOURS: &str = "default_backup_interval_hours";
const DEFAULT_BACKUP_RETAIN_COUNT: &str = "default_backup_retain_count";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InstanceDefaults {
    pub auto_restart: bool,
    pub backup_enabled: bool,
    pub backup_interval_hours: u32,
    pub backup_retain_count: u32,
}

impl Default for InstanceDefaults {
    fn default() -> Self {
        Self {
            auto_restart: false,
            backup_enabled: false,
            backup_interval_hours: 24,
            backup_retain_count: 7,
        }
    }
}

pub fn get(db: &Db, key: &str) -> Result<Option<String>> {
    db.conn()
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .with_context(|| format!("failed to look up setting '{key}'"))
}

pub fn set(db: &Db, key: &str, value: &str) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, chrono::Utc::now()],
        )
        .with_context(|| format!("failed to save setting '{key}'"))?;
    Ok(())
}

pub fn delete(db: &Db, key: &str) -> Result<()> {
    db.conn()
        .execute("DELETE FROM settings WHERE key = ?1", params![key])
        .with_context(|| format!("failed to clear setting '{key}'"))?;
    Ok(())
}

pub fn instance_defaults(db: &Db) -> Result<InstanceDefaults> {
    let fallback = InstanceDefaults::default();
    Ok(InstanceDefaults {
        auto_restart: get(db, DEFAULT_AUTO_RESTART)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback.auto_restart),
        backup_enabled: get(db, DEFAULT_BACKUP_ENABLED)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback.backup_enabled),
        backup_interval_hours: get(db, DEFAULT_BACKUP_INTERVAL_HOURS)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback.backup_interval_hours),
        backup_retain_count: get(db, DEFAULT_BACKUP_RETAIN_COUNT)?
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback.backup_retain_count),
    })
}

pub fn set_instance_defaults(db: &Db, defaults: &InstanceDefaults) -> Result<()> {
    set(db, DEFAULT_AUTO_RESTART, &defaults.auto_restart.to_string())?;
    set(
        db,
        DEFAULT_BACKUP_ENABLED,
        &defaults.backup_enabled.to_string(),
    )?;
    set(
        db,
        DEFAULT_BACKUP_INTERVAL_HOURS,
        &defaults.backup_interval_hours.to_string(),
    )?;
    set(
        db,
        DEFAULT_BACKUP_RETAIN_COUNT,
        &defaults.backup_retain_count.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-settings-test-{label}-{}-{}",
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
        set(&db, NEXUS_API_KEY, "abc123").unwrap();
        assert_eq!(get(&db, NEXUS_API_KEY).unwrap(), Some("abc123".to_string()));
    }

    #[test]
    fn setting_twice_overwrites() {
        let db = temp_db("overwrite");
        set(&db, NEXUS_API_KEY, "first").unwrap();
        set(&db, NEXUS_API_KEY, "second").unwrap();
        assert_eq!(get(&db, NEXUS_API_KEY).unwrap(), Some("second".to_string()));
    }

    #[test]
    fn delete_clears_the_key() {
        let db = temp_db("delete");
        set(&db, NEXUS_API_KEY, "abc123").unwrap();
        delete(&db, NEXUS_API_KEY).unwrap();
        assert!(get(&db, NEXUS_API_KEY).unwrap().is_none());
    }

    #[test]
    fn instance_defaults_round_trip() {
        let db = temp_db("instance-defaults");
        assert_eq!(instance_defaults(&db).unwrap(), InstanceDefaults::default());
        let defaults = InstanceDefaults {
            auto_restart: true,
            backup_enabled: true,
            backup_interval_hours: 6,
            backup_retain_count: 12,
        };
        set_instance_defaults(&db, &defaults).unwrap();
        assert_eq!(instance_defaults(&db).unwrap(), defaults);
    }
}
