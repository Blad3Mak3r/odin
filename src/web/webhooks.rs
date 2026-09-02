//! Background task that forwards activity events to configured outbound
//! webhooks (Discord, or anything else that accepts a `{"content": "..."}`
//! POST body). Subscribes to the same `ActivityLog` broadcast channel the
//! global events SSE already reads from, so this needs no second event bus
//! — every event worth alerting on already flows through it.

use tokio::sync::broadcast::error::RecvError;

use crate::activity::{ActivityEvent, ActivityKind};
use crate::db::webhooks as db_webhooks;
use crate::web::state::AppState;

pub fn spawn(state: AppState) {
    let (_history, mut rx) = state.activity.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => forward(&state, event).await,
                // A slow consumer that missed some events — nothing to
                // retroactively forward, just keep going from here.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
}

async fn forward(state: &AppState, event: ActivityEvent) {
    let db = state.db.clone();
    let hooks = match tokio::task::spawn_blocking(move || db_webhooks::list(&db)).await {
        Ok(Ok(hooks)) => hooks,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "failed to load webhooks");
            return;
        }
        Err(e) => {
            tracing::warn!(error = %e, "webhook lookup task panicked");
            return;
        }
    };

    let tag = kind_tag(&event.kind);
    let content = describe(&event);
    for hook in hooks {
        if !hook.enabled {
            continue;
        }
        if !hook.event_kinds.is_empty() && !hook.event_kinds.contains(&tag) {
            continue;
        }
        let url = hook.url.clone();
        let content = content.clone();
        if let Err(e) = tokio::task::spawn_blocking(move || post(&url, &content)).await {
            tracing::warn!(error = %e, "webhook post task panicked");
        }
    }
}

/// Posts one message to a webhook URL. Blocking — call from
/// `spawn_blocking`.
pub(crate) fn post(url: &str, content: &str) -> anyhow::Result<()> {
    crate::http::CLIENT
        .post(url)
        .json(&serde_json::json!({ "content": content }))
        .send()?
        .error_for_status()?;
    Ok(())
}

/// The same tag serde would put in `ActivityKind`'s `"kind"` field — reused
/// here (rather than a hand-written duplicate list of tags) so a webhook's
/// stored event-kind filter always matches what the API actually emits.
fn kind_tag(kind: &ActivityKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
        .unwrap_or_default()
}

/// A short, human-readable line describing one activity event, suitable as
/// a Discord message's `content`.
fn describe(event: &ActivityEvent) -> String {
    let instance = event.instance.as_deref().unwrap_or("?");
    match &event.kind {
        ActivityKind::InstanceCreated => format!("🆕 Instance created: **{instance}**"),
        ActivityKind::InstanceCloned { source } => {
            format!("🧬 **{instance}** cloned from **{source}**")
        }
        ActivityKind::InstanceDeleted => format!("🗑️ Instance deleted: **{instance}**"),
        ActivityKind::InstanceStarted => format!("▶️ **{instance}** started"),
        ActivityKind::InstanceStopped => format!("⏹️ **{instance}** stopped"),
        ActivityKind::InstanceAutoRestarted => {
            format!("♻️ **{instance}** crashed and was restarted automatically")
        }
        ActivityKind::ServerInstalled => "⬇️ Valheim server files installed/updated".to_string(),
        ActivityKind::ServerUpdateAvailable {
            installed_build_id,
            latest_build_id,
        } => format!(
            "⬆️ Valheim server update available: build {installed_build_id} → {latest_build_id}"
        ),
        ActivityKind::ModInstalled { mod_id } => {
            format!("📦 Mod installed on **{instance}**: {mod_id}")
        }
        ActivityKind::ModRemoved { mod_id } => {
            format!("📦 Mod removed from **{instance}**: {mod_id}")
        }
        ActivityKind::ModsUpdated => format!("📦 Mods updated on **{instance}**"),
        ActivityKind::BepInExUpdated {
            from_version,
            to_version,
        } => format!(
            "⬆️ BepInEx updated on **{instance}**: {} → {to_version}",
            from_version.as_deref().unwrap_or("unknown")
        ),
        ActivityKind::BackupCreated { backup_id } => {
            format!("💾 Backup created for **{instance}**: {backup_id}")
        }
        ActivityKind::BackupRestored { backup_id } => {
            format!("♻️ **{instance}** restored from backup {backup_id}")
        }
        ActivityKind::BackupPruned { backup_id } => {
            format!("🧹 Old backup pruned for **{instance}**: {backup_id}")
        }
        ActivityKind::PlayerJoined { name } => format!("👋 {name} joined **{instance}**"),
        ActivityKind::PlayerLeft { name } => format!("👋 {name} left **{instance}**"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn kind_tag_matches_the_serialized_json_tag() {
        assert_eq!(kind_tag(&ActivityKind::InstanceStopped), "instance_stopped");
        assert_eq!(
            kind_tag(&ActivityKind::ModInstalled {
                mod_id: "owner-mod".to_string()
            }),
            "mod_installed"
        );
    }

    #[test]
    fn describe_mentions_the_instance_name() {
        let event = ActivityEvent {
            id: "evt-1".to_string(),
            at: Utc::now(),
            game: crate::game::GameId::Valheim,
            instance: Some("my-server".to_string()),
            instance_id: None,
            kind: ActivityKind::InstanceStopped,
        };
        assert!(describe(&event).contains("my-server"));
    }
}
