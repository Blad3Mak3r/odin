//! Background task that forwards activity events to configured outbound
//! webhooks (Discord, or anything else that accepts a `{"content": "..."}`
//! POST body). Subscribes to the same `ActivityLog` broadcast channel the
//! global events SSE already reads from, so this needs no second event bus
//! — every event worth alerting on already flows through it.

use std::time::Duration;

use tokio::sync::broadcast::error::RecvError;

use crate::activity::{ActivityEvent, ActivityKind};
use crate::db::Db;
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
        // One task per delivery, so a slow or failing destination retrying
        // in the background never delays the others or the next event.
        tokio::spawn(deliver(
            state.db.clone(),
            hook.id.clone(),
            hook.url.clone(),
            content.clone(),
        ));
    }
}

/// Delay before each retry of a failed delivery; its length is the retry
/// budget.
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(4),
    Duration::from_secs(16),
];

const POST_TIMEOUT: Duration = Duration::from_secs(10);

async fn deliver(db: std::sync::Arc<Db>, id: String, url: String, content: String) {
    let outcome = deliver_with(&url, &content, &RETRY_DELAYS).await;
    if let Err(reason) = &outcome {
        tracing::warn!(webhook = %id, %reason, "webhook delivery failed");
    }
    let recorded = tokio::task::spawn_blocking(move || {
        db_webhooks::record_delivery(&db, &id, outcome.err().as_deref())
    })
    .await;
    match recorded {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "failed to record webhook delivery"),
        Err(error) => tracing::warn!(%error, "webhook delivery record task panicked"),
    }
}

/// Posts `content`, retrying transient failures after each of `delays`.
/// The error says what went wrong without the URL, which is the webhook's
/// secret.
async fn deliver_with(url: &str, content: &str, delays: &[Duration]) -> Result<(), String> {
    let mut retries = delays.iter();
    loop {
        let (post_url, post_content) = (url.to_string(), content.to_string());
        let error = match tokio::task::spawn_blocking(move || post(&post_url, &post_content)).await
        {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => e,
            Err(_) => return Err("delivery task panicked".to_string()),
        };
        match retries.next() {
            Some(delay) if is_transient(&error) => tokio::time::sleep(*delay).await,
            _ => return Err(public_reason(&error)),
        }
    }
}

/// A short description of a failed post that never includes the URL, which
/// `reqwest` would otherwise put in its message.
pub(crate) fn public_reason(error: &anyhow::Error) -> String {
    match error.downcast_ref::<reqwest::Error>() {
        Some(e) => match e.status() {
            Some(status) => format!("HTTP {}", status.as_u16()),
            None if e.is_timeout() => "timed out".to_string(),
            None if e.is_connect() => "could not connect".to_string(),
            None => "request failed".to_string(),
        },
        None => "request failed".to_string(),
    }
}

/// Whether a failed post is worth retrying: network trouble, a timeout, a
/// server error or rate limiting. A 4xx such as a revoked URL will not fix
/// itself.
fn is_transient(error: &anyhow::Error) -> bool {
    match error.downcast_ref::<reqwest::Error>() {
        Some(e) => e
            .status()
            .is_none_or(|s| s.is_server_error() || s == reqwest::StatusCode::TOO_MANY_REQUESTS),
        None => false,
    }
}

/// Posts one message to a webhook URL. Blocking — call from
/// `spawn_blocking`.
pub(crate) fn post(url: &str, content: &str) -> anyhow::Result<()> {
    crate::http::CLIENT
        .post(url)
        .timeout(POST_TIMEOUT)
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
        ActivityKind::ServerInstalled => {
            format!(
                "⬇️ {} server files installed/updated",
                game_name(event.game)
            )
        }
        ActivityKind::ServerUpdateAvailable {
            installed_build_id,
            latest_build_id,
        } => format!(
            "⬆️ {} server update available: build {installed_build_id} → {latest_build_id}",
            game_name(event.game)
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
        ActivityKind::MapWiped => format!("💥 Map wiped for **{instance}**"),
        ActivityKind::FullWiped => format!("💥 Full wipe completed for **{instance}**"),
        ActivityKind::JobFailed { job, message } => {
            let message: String = message.chars().take(500).collect();
            format!("❌ Job `{job}` failed for **{instance}**: {message}")
        }
        ActivityKind::StartupFailed { reason } => {
            let reason: String = reason.chars().take(500).collect();
            format!("🚨 **{instance}** failed to start automatically: {reason}")
        }
        ActivityKind::PlayerJoined { name } => format!("👋 {name} joined **{instance}**"),
        ActivityKind::PlayerLeft { name } => format!("👋 {name} left **{instance}**"),
    }
}

fn game_name(game: crate::game::GameId) -> &'static str {
    match game {
        crate::game::GameId::ArkSurvivalAscended => "ARK: Survival Ascended",
        crate::game::GameId::Valheim => "Valheim",
        crate::game::GameId::Rust => "Rust",
        crate::game::GameId::Palworld => "Palworld",
        crate::game::GameId::SevenDaysToDie => "7 Days to Die",
        crate::game::GameId::Cs2 => "Counter-Strike 2",
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

    /// Serves one canned response per connection, in order, and returns the
    /// URL plus a counter of the requests received.
    fn serve(statuses: &'static [u16]) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let count = std::sync::Arc::new(AtomicUsize::new(0));
        let served = count.clone();
        std::thread::spawn(move || {
            for status in statuses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                // Read the head and the body so the client sees a clean close.
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let read = stream.read(&mut chunk).unwrap();
                    request.extend_from_slice(&chunk[..read]);
                    let text = String::from_utf8_lossy(&request).to_lowercase();
                    if let Some(head_end) = text.find("\r\n\r\n") {
                        let length = text
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= head_end + 4 + length {
                            break;
                        }
                    }
                    if read == 0 {
                        break;
                    }
                }
                served.fetch_add(1, Ordering::SeqCst);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
            }
        });
        (url, count)
    }

    const NO_WAIT: [Duration; 3] = [Duration::ZERO; 3];

    #[tokio::test]
    async fn a_server_error_is_retried_until_it_succeeds() {
        let (url, count) = serve(&[500, 429, 204]);

        assert_eq!(deliver_with(&url, "hello", &NO_WAIT).await, Ok(()));

        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_client_error_is_not_retried() {
        let (url, count) = serve(&[404, 204]);

        assert_eq!(
            deliver_with(&url, "hello", &NO_WAIT).await,
            Err("HTTP 404".to_string())
        );

        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retries_stop_after_the_budget_is_spent() {
        let (url, count) = serve(&[503, 503, 503, 503, 503]);

        assert_eq!(
            deliver_with(&url, "hello", &NO_WAIT).await,
            Err("HTTP 503".to_string())
        );

        // The first attempt plus one retry per delay.
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn the_outcome_of_a_delivery_is_recorded_without_the_url() {
        let dir = std::env::temp_dir().join(format!(
            "odin-webhook-delivery-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = std::sync::Arc::new(
            Db::open(&crate::paths::Paths {
                data_dir: dir.clone(),
                config_dir: dir,
            })
            .unwrap(),
        );
        let (url, _count) = serve(&[404, 204]);
        let hook = db_webhooks::insert(&db, &url, &[]).unwrap();

        deliver(
            db.clone(),
            hook.id.clone(),
            url.clone(),
            "hello".to_string(),
        )
        .await;
        let failed = db_webhooks::get(&db, &hook.id)
            .unwrap()
            .unwrap()
            .last_delivery;
        let error = failed.unwrap().error.unwrap();
        assert_eq!(error, "HTTP 404");
        assert!(!error.contains(&url));

        deliver(db.clone(), hook.id.clone(), url, "hello".to_string()).await;
        let delivered = db_webhooks::get(&db, &hook.id)
            .unwrap()
            .unwrap()
            .last_delivery;
        assert!(delivered.unwrap().error.is_none());
    }

    #[test]
    fn failures_are_described_for_the_webhook() {
        let event = |kind| ActivityEvent {
            id: "evt-1".to_string(),
            at: Utc::now(),
            game: crate::game::GameId::Palworld,
            instance: Some("pals".to_string()),
            instance_id: None,
            kind,
        };
        let job = describe(&event(ActivityKind::JobFailed {
            job: "backup_create".to_string(),
            message: "x".repeat(2000),
        }));
        assert!(job.contains("backup_create") && job.contains("pals"));
        assert!(job.chars().count() < 600);
        let startup = describe(&event(ActivityKind::StartupFailed {
            reason: "port in use".to_string(),
        }));
        assert!(startup.contains("pals") && startup.contains("port in use"));
        assert_eq!(
            kind_tag(&ActivityKind::StartupFailed {
                reason: String::new()
            }),
            "startup_failed"
        );
    }

    #[test]
    fn describe_uses_the_event_game_for_server_installations() {
        let event = ActivityEvent {
            id: "evt-1".to_string(),
            at: Utc::now(),
            game: crate::game::GameId::Rust,
            instance: None,
            instance_id: None,
            kind: ActivityKind::ServerInstalled,
        };
        assert!(describe(&event).contains("Rust"));
    }
}
