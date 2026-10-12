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
