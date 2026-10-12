//! Explicit migration marker for games running in Podman.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{OptionalExtension, params};

use super::Db;

pub fn is_podman(db: &Db, instance_id: &str) -> Result<bool> {
    db.conn()
        .query_row(
            "SELECT 1 FROM podman_instances WHERE instance_id = ?1",
            params![instance_id],
            |_| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .context("failed to load Podman migration state")
}

pub fn mark_podman(db: &Db, instance_id: &str) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO podman_instances (instance_id, migrated_at) VALUES (?1, ?2) \
             ON CONFLICT(instance_id) DO NOTHING",
            params![instance_id, Utc::now()],
        )
        .context("failed to mark instance as Podman-managed")?;
    Ok(())
}

/// Whether an operator (or Odin acting for one) last left the instance
/// started. False for an instance that was never migrated.
pub fn desired_running(db: &Db, instance_id: &str) -> Result<bool> {
    db.conn()
        .query_row(
            "SELECT desired_running FROM podman_instances WHERE instance_id = ?1",
            params![instance_id],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .map(|row| row.unwrap_or(false))
        .context("failed to load Podman desired state")
}

pub fn set_desired_running(db: &Db, instance_id: &str, desired: bool) -> Result<()> {
    db.conn()
        .execute(
            "UPDATE podman_instances SET desired_running = ?2 WHERE instance_id = ?1",
            params![instance_id, desired],
        )
        .context("failed to record Podman desired state")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[test]
    fn desired_state_follows_the_marker_and_defaults_to_stopped() {
        let dir = std::env::temp_dir().join(format!("odin-desired-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let db = Db::open(&paths).unwrap();
        let instance =
            crate::game::instances::create(&paths, &db, crate::game::GameId::Rust, "rusty")
                .unwrap();
        let crate::game::instances::GameInstance::Rust(instance) = instance else {
            unreachable!("created a Rust instance")
        };
        let id = instance.identity.id;

        assert!(!desired_running(&db, &id).unwrap());
        set_desired_running(&db, &id, true).unwrap();
        assert!(desired_running(&db, &id).unwrap());
        set_desired_running(&db, &id, false).unwrap();
        assert!(!desired_running(&db, &id).unwrap());
        assert!(!desired_running(&db, "never-migrated").unwrap());
        std::fs::remove_dir_all(dir).ok();
    }
}
