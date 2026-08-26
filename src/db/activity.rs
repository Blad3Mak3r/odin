//! Durable storage for the activity feed, backing `crate::activity::ActivityLog`.

use anyhow::{Context, Result};
use rusqlite::params;

use super::Db;
use crate::activity::{ActivityEvent, ActivityKind};

/// Appends one event. Call from a blocking context.
pub fn insert(db: &Db, event: &ActivityEvent) -> Result<()> {
    let (kind, payload) = encode_kind(&event.kind)?;
    db.conn().execute(
        "INSERT INTO activity_events (id, at, instance, kind, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![event.id, event.at, event.instance, kind, payload],
    )?;
    Ok(())
}

/// Returns up to `limit` most recent events, oldest first (matching the
/// order the in-memory ring buffer keeps them in).
pub fn recent(db: &Db, limit: usize) -> Result<Vec<ActivityEvent>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, at, instance, payload FROM \
         (SELECT rowid, id, at, instance, payload FROM activity_events ORDER BY rowid DESC LIMIT ?1) \
         ORDER BY rowid ASC",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        let payload: String = row.get(3)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, chrono::DateTime<chrono::Utc>>(1)?,
            row.get::<_, Option<String>>(2)?,
            payload,
        ))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (id, at, instance, payload) = row?;
        let kind = decode_kind(&payload)?;
        events.push(ActivityEvent {
            id,
            at,
            instance,
            kind,
        });
    }
    Ok(events)
}

fn encode_kind(kind: &ActivityKind) -> Result<(String, String)> {
    let value = serde_json::to_value(kind).context("failed to serialize activity kind")?;
    let tag = value
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((tag, value.to_string()))
}

fn decode_kind(payload: &str) -> Result<ActivityKind> {
    serde_json::from_str(payload).context("failed to deserialize activity payload")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-activity-test-{label}-{}-{}",
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
    fn insert_then_recent_round_trips() {
        let db = temp_db("roundtrip");
        let event = ActivityEvent {
            id: "evt-1".to_string(),
            at: chrono::Utc::now(),
            instance: Some("my-server".to_string()),
            kind: ActivityKind::ModInstalled {
                mod_id: "owner-mod".to_string(),
            },
        };
        insert(&db, &event).unwrap();

        let events = recent(&db, 200).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "evt-1");
        assert_eq!(events[0].instance.as_deref(), Some("my-server"));
        assert!(matches!(events[0].kind, ActivityKind::ModInstalled { .. }));
    }

    #[test]
    fn recent_respects_limit_and_order() {
        let db = temp_db("limit");
        for i in 0..5 {
            insert(
                &db,
                &ActivityEvent {
                    id: format!("evt-{i}"),
                    at: chrono::Utc::now(),
                    instance: None,
                    kind: ActivityKind::InstanceCreated,
                },
            )
            .unwrap();
        }

        let events = recent(&db, 3).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].id, "evt-2");
        assert_eq!(events[1].id, "evt-3");
        assert_eq!(events[2].id, "evt-4");
    }
}
