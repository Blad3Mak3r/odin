//! Login, logout, first-run setup, and self-service password change.

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::db::users::{self, User};
use crate::web::auth::{
    AuthUser, InvalidCredentials, authenticate, clear_cookie, hash_password, is_secure,
    start_session, token_hash_of, validate_password, validate_username, verify_password,
};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct StatusResponse {
    pub setup_required: bool,
    pub user: Option<User>,
}

pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<StatusResponse>> {
    let db = state.db.clone();
    let setup_required = run_blocking(move || users::count(&db)).await? == 0;
    let user = authenticate(&state, &headers).await?.map(|a| a.user);
    Ok(Json(StatusResponse {
        setup_required,
        user,
    }))
}

#[derive(Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

fn with_cookie(cookie: String, body: impl IntoResponse) -> Response {
    let mut response = body.into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

pub async fn setup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<Credentials>,
) -> ApiResult<Response> {
    validate_username(&req.username)?;
    validate_password(&req.password)?;
    let db = state.db.clone();
    let user = run_blocking(move || {
        let hash = hash_password(&req.password)?;
        users::create_first_admin(&db, &req.username, &hash)
    })
    .await?;
    let cookie = start_session(&state, &user.id, is_secure(&headers)).await?;
    Ok(with_cookie(cookie, Json(user)))
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<Credentials>,
) -> ApiResult<Response> {
    state.login_throttle.check(&req.username)?;
    let db = state.db.clone();
    let (username, password) = (req.username.clone(), req.password);
    let user = run_blocking(move || {
        let found = users::find_credentials(&db, &username)?;
        // Verify against a dummy hash for unknown users so timing does not
        // reveal which usernames exist.
        let stored = found
            .as_ref()
            .map_or(dummy_hash(), |c| c.password_hash.as_str());
        let ok = verify_password(&password, stored);
        Ok(found.filter(|_| ok).map(|c| c.user))
    })
    .await?;
    let Some(user) = user else {
        state.login_throttle.record_failure(&req.username);
        return Err(InvalidCredentials.into());
    };
    state.login_throttle.clear(&req.username);
    let cookie = start_session(&state, &user.id, is_secure(&headers)).await?;
    Ok(with_cookie(cookie, Json(user)))
}

/// A real hash of a throwaway password, so unknown usernames cost the same
/// as known ones.
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password("odin-dummy-password").unwrap_or_default())
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token_hash) = token_hash_of(&headers) {
        let db = state.db.clone();
        run_blocking(move || users::delete_session(&db, &token_hash)).await?;
    }
    Ok(with_cookie(
        clear_cookie(is_secure(&headers)),
        StatusCode::NO_CONTENT,
    ))
}

#[derive(Deserialize)]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    headers: HeaderMap,
    Json(req): Json<ChangePassword>,
) -> ApiResult<Response> {
    validate_password(&req.new_password)?;
    let (db, id) = (state.db.clone(), auth.user.id.clone());
    run_blocking(move || {
        let stored = users::password_hash(&db, &id)?.unwrap_or_default();
        if !verify_password(&req.current_password, &stored) {
            return Err(InvalidCredentials.into());
        }
        // Revokes every session, including this one; a fresh one follows.
        users::set_password(&db, &id, &hash_password(&req.new_password)?)
    })
    .await?;
    let cookie = start_session(&state, &auth.user.id, is_secure(&headers)).await?;
    Ok(with_cookie(cookie, StatusCode::NO_CONTENT))
}
