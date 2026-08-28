//! Durable storage for outbound webhooks (Discord or Discord-compatible)
//! that get a message when a configured kind of activity event happens,
//! backing `web::webhooks`'s background forwarder.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use thiserror::Error;
use uuid::Uuid;

use super::Db;

/// A failure a caller (the web API) may want to distinguish from other,
/// unexpected errors.
#[derive(Debug, Error)]
pub enum WebhookError {
    #[error("webhook '{0}' not found")]
    NotFound(String),
}

#[derive(Debug, Clone)]
pub struct WebhookRow {
    pub id: String,
    pub url: String,
    pub enabled: bool,
    /// Activity kind tags (e.g. `"instance_stopped"`) this webhook forwards.
    /// Empty means "every kind" — the default for a newly created webhook,
    /// so turning one on doesn't also require picking which events matter
    /// first.
    pub event_kinds: Vec<String>,
    pub created_at: DateTime<Utc>,
}

pub fn list(db: &Db) -> Result<Vec<WebhookRow>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, url, enabled, event_kinds, created_at FROM webhooks ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], row_to_webhook)?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

pub fn get(db: &Db, id: &str) -> Result<Option<WebhookRow>> {
    db.conn()
        .query_row(
            "SELECT id, url, enabled, event_kinds, created_at FROM webhooks WHERE id = ?1",
            params![id],
            row_to_webhook,
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert(db: &Db, url: &str, event_kinds: &[String]) -> Result<WebhookRow> {
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now();
    let kinds_json = serde_json::to_string(event_kinds)?;
    db.conn().execute(
        "INSERT INTO webhooks (id, url, enabled, event_kinds, created_at) VALUES (?1, ?2, 1, ?3, ?4)",
        params![id, url, kinds_json, created_at],
    )?;
    Ok(WebhookRow {
        id,
        url: url.to_string(),
        enabled: true,
        event_kinds: event_kinds.to_vec(),
        created_at,
    })
}

/// A no-op if the id doesn't exist.
pub fn delete(db: &Db, id: &str) -> Result<()> {
    db.conn()
        .execute("DELETE FROM webhooks WHERE id = ?1", params![id])?;
    Ok(())
}

/// A no-op if the id doesn't exist.
pub fn set_enabled(db: &Db, id: &str, enabled: bool) -> Result<()> {
    db.conn().execute(
        "UPDATE webhooks SET enabled = ?2 WHERE id = ?1",
        params![id, enabled],
    )?;
    Ok(())
}

fn row_to_webhook(row: &rusqlite::Row) -> rusqlite::Result<WebhookRow> {
    let kinds_json: String = row.get(3)?;
    Ok(WebhookRow {
        id: row.get(0)?,
        url: row.get(1)?,
        enabled: row.get(2)?,
        event_kinds: serde_json::from_str(&kinds_json).unwrap_or_default(),
        created_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-webhooks-test-{label}-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        })
        .unwrap()
    }

    #[test]
    fn insert_then_list_round_trips() {
        let db = temp_db("roundtrip");
        let created = insert(&db, "https://discord.com/api/webhooks/1/abc", &[]).unwrap();
        assert!(created.enabled);
        assert!(created.event_kinds.is_empty());

        let hooks = list(&db).unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].id, created.id);
        assert_eq!(hooks[0].url, "https://discord.com/api/webhooks/1/abc");
    }

    #[test]
    fn insert_preserves_event_kind_filter() {
        let db = temp_db("filter");
        let created = insert(
            &db,
            "https://discord.com/api/webhooks/1/abc",
            &["instance_stopped".to_string(), "backup_created".to_string()],
        )
        .unwrap();

        let hook = get(&db, &created.id).unwrap().unwrap();
        assert_eq!(
            hook.event_kinds,
            vec!["instance_stopped".to_string(), "backup_created".to_string()]
        );
    }

    #[test]
    fn set_enabled_toggles_without_losing_config() {
        let db = temp_db("toggle");
        let created = insert(&db, "https://discord.com/api/webhooks/1/abc", &[]).unwrap();

        set_enabled(&db, &created.id, false).unwrap();
        let disabled = get(&db, &created.id).unwrap().unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.url, created.url);
    }

    #[test]
    fn delete_removes_the_row() {
        let db = temp_db("delete");
        let created = insert(&db, "https://discord.com/api/webhooks/1/abc", &[]).unwrap();

        delete(&db, &created.id).unwrap();

        assert!(get(&db, &created.id).unwrap().is_none());
    }

    #[test]
    fn get_missing_id_is_none() {
        let db = temp_db("missing");
        assert!(get(&db, "nope").unwrap().is_none());
    }
}
