use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::db::webhooks::{self, WebhookError, WebhookRow};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct WebhookView {
    pub id: String,
    pub url: String,
    pub enabled: bool,
    pub event_kinds: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl From<WebhookRow> for WebhookView {
    fn from(row: WebhookRow) -> Self {
        Self {
            id: row.id,
            url: row.url,
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

pub async fn create_webhook(
    State(state): State<AppState>,
    Json(req): Json<CreateWebhookRequest>,
) -> ApiResult<Json<WebhookView>> {
    if !(req.url.starts_with("http://") || req.url.starts_with("https://")) {
        return Err(BadRequest("url must start with http:// or https://".to_string()).into());
    }

    let db = state.db.clone();
    let row = run_blocking(move || webhooks::insert(&db, &req.url, &req.event_kinds)).await?;
    Ok(Json(row.into()))
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
