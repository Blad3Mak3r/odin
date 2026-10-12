//! Explicit migration marker for games running in Podman.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use super::Db;

/// Whether an operator (or Odin acting for one) last left the instance
/// started. False for an instance that has never been started.
pub fn desired_running(db: &Db, instance_id: &str) -> Result<bool> {
    db.conn()
        .query_row(
            "SELECT desired_running FROM instance_runtime_state WHERE instance_id = ?1",
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
            "INSERT INTO instance_runtime_state (instance_id, desired_running) VALUES (?1, ?2) \
             ON CONFLICT(instance_id) DO UPDATE SET desired_running = excluded.desired_running",
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
    fn desired_state_defaults_to_stopped_and_follows_the_last_request() {
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
        assert!(!desired_running(&db, "never-started").unwrap());
        std::fs::remove_dir_all(dir).ok();
    }
}
