use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::db::webhooks::{self, WebhookError, WebhookRow};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

const WEBHOOK_EVENT_KINDS: &[&str] = &[
    "instance_created",
    "instance_deleted",
    "instance_started",
    "instance_stopped",
    "instance_auto_restarted",
    "server_installed",
    "server_update_available",
    "mod_installed",
    "mod_removed",
    "mods_updated",
    "bepinex_updated",
    "backup_created",
    "backup_restored",
    "backup_pruned",
    "player_joined",
    "player_left",
];

#[derive(Serialize)]
pub struct WebhookView {
    pub id: String,
    pub enabled: bool,
    pub event_kinds: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl From<WebhookRow> for WebhookView {
    fn from(row: WebhookRow) -> Self {
        Self {
            id: row.id,
            enabled: row.enabled,
            event_kinds: row.event_kinds,
            created_at: row.created_at,
        }
    }
}

pub async fn list_webhooks(State(state): State<AppState>) -> ApiResult<Json<Vec<WebhookView>>> {
    let db = state.db.clone();
    let hooks = run_blocking(move || webhooks::list(&db)).await?;
    Ok(Json(hooks.into_iter().map(Into::into).collect()))
}

#[derive(Deserialize)]
pub struct CreateWebhookRequest {
    pub url: String,
    #[serde(default)]
    pub event_kinds: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateWebhookRequest {
    pub event_kinds: Vec<String>,
}

pub async fn create_webhook(
    State(state): State<AppState>,
    Json(req): Json<CreateWebhookRequest>,
) -> ApiResult<Json<WebhookView>> {
    if !(req.url.starts_with("http://") || req.url.starts_with("https://")) {
        return Err(BadRequest("url must start with http:// or https://".to_string()).into());
    }
    validate_event_kinds(&req.event_kinds)?;

    let db = state.db.clone();
    let row = run_blocking(move || webhooks::insert(&db, &req.url, &req.event_kinds)).await?;
    Ok(Json(row.into()))
}

pub async fn update_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateWebhookRequest>,
) -> ApiResult<StatusCode> {
    validate_event_kinds(&req.event_kinds)?;

    let db = state.db.clone();
    let lookup_id = id.clone();
    let updated =
        run_blocking(move || webhooks::set_event_kinds(&db, &lookup_id, &req.event_kinds)).await?;
    if !updated {
        return Err(WebhookError::NotFound(id).into());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let db = state.db.clone();
    run_blocking(move || webhooks::delete(&db, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn enable_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let db = state.db.clone();
    run_blocking(move || webhooks::set_enabled(&db, &id, true)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn disable_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let db = state.db.clone();
    run_blocking(move || webhooks::set_enabled(&db, &id, false)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn test_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let db = state.db.clone();
    let lookup_id = id.clone();
    let hook = run_blocking(move || webhooks::get(&db, &lookup_id)).await?;
    let Some(hook) = hook else {
        return Err(WebhookError::NotFound(id).into());
    };

    run_blocking(move || {
        crate::web::webhooks::post(
            &hook.url,
            "✅ Test message from Odin — this webhook is working.",
        )
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn validate_event_kinds(event_kinds: &[String]) -> Result<(), BadRequest> {
    if let Some(kind) = event_kinds
        .iter()
        .find(|kind| !WEBHOOK_EVENT_KINDS.contains(&kind.as_str()))
    {
        return Err(BadRequest(format!("unknown webhook event kind: {kind}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;
    use crate::db::Db;
    use crate::paths::Paths;

    fn test_app(label: &str) -> (axum::Router, Arc<Db>) {
        let dir = std::env::temp_dir().join(format!(
            "odin-webhook-route-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let app = crate::web::router::build_router(AppState::new(paths, db.clone()));
        (app, db)
    }

    #[tokio::test]
    async fn webhook_views_do_not_serialize_urls() {
        let (app, db) = test_app("redacted-view");
        webhooks::insert(&db, "https://discord.com/api/webhooks/1/secret-token", &[]).unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/webhooks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(!body.contains("secret-token"));
        assert!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()[0]
                .get("url")
                .is_none()
        );
    }

    #[tokio::test]
    async fn update_webhook_rejects_unknown_event_kinds() {
        let (app, db) = test_app("invalid-kind");
        let webhook =
            webhooks::insert(&db, "https://discord.com/api/webhooks/1/token", &[]).unwrap();
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/webhooks/{}", webhook.id))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"event_kinds":["not_an_event"]}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn update_webhook_replaces_event_kinds() {
        let (app, db) = test_app("update");
        let webhook = webhooks::insert(
            &db,
            "https://discord.com/api/webhooks/1/token",
            &["instance_started".to_string()],
        )
        .unwrap();
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/webhooks/{}", webhook.id))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"event_kinds":["backup_created"]}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let updated = webhooks::get(&db, &webhook.id).unwrap().unwrap();
        assert_eq!(updated.event_kinds, vec!["backup_created"]);
        assert_eq!(updated.url, webhook.url);
        assert_eq!(updated.enabled, webhook.enabled);
    }

    #[tokio::test]
    async fn update_webhook_returns_not_found_for_missing_webhook() {
        let (app, _) = test_app("missing");
        let request = Request::builder()
            .method("PUT")
            .uri("/api/webhooks/missing")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"event_kinds":[]}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
