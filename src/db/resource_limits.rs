//! Per-instance CPU and memory limits shared by every game driver.

use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::Db;

/// A missing value leaves that resource unlimited.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub cpu_percent: Option<f64>,
    pub memory_max_bytes: Option<u64>,
}

impl ResourceLimits {
    pub fn is_unlimited(&self) -> bool {
        self.cpu_percent.is_none() && self.memory_max_bytes.is_none()
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(cpu_percent) = self.cpu_percent
            && (!cpu_percent.is_finite() || cpu_percent <= 0.0)
        {
            bail!("cpu_percent must be a finite number greater than zero");
        }
        if matches!(self.memory_max_bytes, Some(0)) {
            bail!("memory_max_bytes must be greater than zero");
        }
        Ok(())
    }
}

pub fn load(db: &Db, instance_id: &str) -> Result<ResourceLimits> {
    db.conn()
        .query_row(
            "SELECT cpu_percent, memory_max_bytes FROM instance_resource_limits WHERE instance_id = ?1",
            params![instance_id],
            |row| {
                Ok(ResourceLimits {
                    cpu_percent: row.get(0)?,
                    memory_max_bytes: row.get(1)?,
                })
            },
        )
        .optional()
        .map(|limits| limits.unwrap_or_default())
        .context("failed to load instance resource limits")
}

pub fn save(db: &Db, instance_id: &str, limits: &ResourceLimits) -> Result<ResourceLimits> {
    limits.validate()?;
    let conn = db.conn();
    if limits.is_unlimited() {
        conn.execute(
            "DELETE FROM instance_resource_limits WHERE instance_id = ?1",
            params![instance_id],
        )
        .context("failed to clear instance resource limits")?;
    } else {
        conn.execute(
            "INSERT INTO instance_resource_limits (instance_id, cpu_percent, memory_max_bytes) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(instance_id) DO UPDATE SET \
               cpu_percent = excluded.cpu_percent, memory_max_bytes = excluded.memory_max_bytes",
            params![instance_id, limits.cpu_percent, limits.memory_max_bytes],
        )
        .context("failed to save instance resource limits")?;
    }
    Ok(limits.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn db() -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-resource-limits-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        db.conn().execute(
            "INSERT INTO game_instances (id, game, name, created_at) VALUES ('one', 'rust', 'one', '2026-01-01T00:00:00Z')",
            [],
        ).unwrap();
        db
    }

    #[test]
    fn missing_limits_are_unlimited_and_can_be_cleared() {
        let db = db();
        assert_eq!(load(&db, "one").unwrap(), ResourceLimits::default());
        let limits = ResourceLimits {
            cpu_percent: Some(125.5),
            memory_max_bytes: Some(2 * 1024 * 1024 * 1024),
        };
        save(&db, "one", &limits).unwrap();
        assert_eq!(load(&db, "one").unwrap(), limits);
        save(&db, "one", &ResourceLimits::default()).unwrap();
        assert_eq!(load(&db, "one").unwrap(), ResourceLimits::default());
    }

    #[test]
    fn invalid_limits_are_rejected() {
        assert!(
            ResourceLimits {
                cpu_percent: Some(0.0),
                memory_max_bytes: None
            }
            .validate()
            .is_err()
        );
        assert!(
            ResourceLimits {
                cpu_percent: Some(f64::NAN),
                memory_max_bytes: None
            }
            .validate()
            .is_err()
        );
        assert!(
            ResourceLimits {
                cpu_percent: None,
                memory_max_bytes: Some(0)
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn deleting_an_instance_cascades_its_limits() {
        let db = db();
        save(
            &db,
            "one",
            &ResourceLimits {
                cpu_percent: Some(100.0),
                memory_max_bytes: None,
            },
        )
        .unwrap();
        db.conn()
            .execute("DELETE FROM game_instances WHERE id = 'one'", [])
            .unwrap();
        let count: u32 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM instance_resource_limits", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
