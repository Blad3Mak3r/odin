//! First-run setup, recovery, login, logout, password change, and the
//! caller's own session list.

use std::net::IpAddr;

use axum::Json;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use crate::db::auth_events::{self, AuthEvent};
use crate::db::users::{self, SessionInfo, User, UserError};
use crate::web::audit::{self, AuditKind};
use crate::web::auth::{
    AuthUser, BootstrapKind, ClientIp, InvalidBootstrapToken, InvalidCredentials, authenticate,
    clear_cookie, hash_password, is_secure, normalize_email, optional_username, random_token,
    session_origin, start_session, token_hash_of, validate_password, verify_password,
};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct StatusResponse {
    pub setup_required: bool,
    /// An operator asked for recovery; the login page offers the recovery form.
    pub recovery_active: bool,
    pub user: Option<User>,
    /// Echo in `X-CSRF-Token` on every non-GET request.
    pub csrf_token: Option<String>,
}

pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Json<StatusResponse>> {
    let db = state.db.clone();
    let setup_required = run_blocking(move || users::count(&db)).await? == 0;
    let auth = authenticate(&state, &headers).await?;
    Ok(Json(StatusResponse {
        setup_required,
        recovery_active: state.bootstrap.kind() == Some(BootstrapKind::Recovery),
        csrf_token: auth.as_ref().map(|a| a.csrf_token.clone()),
        user: auth.map(|a| a.user),
    }))
}

fn with_cookie(cookie: String, body: impl IntoResponse) -> Response {
    let mut response = body.into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

/// Checks the one-time token, counting wrong guesses against the caller.
async fn check_bootstrap_token(
    state: &AppState,
    kind: BootstrapKind,
    token: &str,
    ip: Option<IpAddr>,
) -> ApiResult<()> {
    let detail = format!("{kind:?}").to_lowercase();
    if let Err(throttled) = state.login_throttle.check("bootstrap", ip) {
        audit::record(
            state,
            AuditKind::LoginThrottled,
            None,
            None,
            ip,
            Some(&detail),
        )
        .await;
        return Err(throttled.into());
    }
    if state.bootstrap.matches(kind, token) {
        return Ok(());
    }
    state.login_throttle.record_failure("bootstrap", ip);
    tracing::warn!(?ip, ?kind, "wrong dashboard bootstrap token");
    audit::record(
        state,
        AuditKind::BootstrapTokenFailed,
        None,
        None,
        ip,
        Some(&detail),
    )
    .await;
    Err(InvalidBootstrapToken.into())
}

#[derive(Deserialize)]
pub struct SetupRequest {
    /// The token printed in the server log at startup.
    pub token: String,
    pub email: String,
    #[serde(default)]
    pub username: Option<String>,
    pub password: String,
}

pub async fn setup(
    State(state): State<AppState>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> ApiResult<Response> {
    let email = normalize_email(&req.email)?;
    let username = optional_username(req.username.as_deref())?;
    validate_password(&req.password)?;
    let db = state.db.clone();
    if run_blocking(move || users::count(&db)).await? > 0 {
        return Err(UserError::SetupDone.into());
    }
    check_bootstrap_token(&state, BootstrapKind::Setup, &req.token, ip).await?;
    let db = state.db.clone();
    let user = run_blocking(move || {
        let hash = hash_password(&req.password)?;
        users::create_first_admin(&db, &email, username.as_deref(), &hash)
    })
    .await?;
    state.bootstrap.disarm();
    audit::record(
        &state,
        AuditKind::SetupCompleted,
        Some(&user.email),
        None,
        ip,
        None,
    )
    .await;
    let origin = session_origin(&headers, ip, true);
    let cookie = start_session(&state, &user.email, is_secure(&state, &headers), origin).await?;
    Ok(with_cookie(cookie, Json(user)))
}

#[derive(Deserialize)]
pub struct RecoverRequest {
    pub token: String,
    pub email: String,
    pub password: String,
}

/// Sets a new password for an existing account, authorised by the recovery
/// token an operator armed by restarting Odin with the marker file present.
pub async fn recover(
    State(state): State<AppState>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    headers: HeaderMap,
    Json(req): Json<RecoverRequest>,
) -> ApiResult<Response> {
    let email = normalize_email(&req.email)?;
    validate_password(&req.password)?;
    check_bootstrap_token(&state, BootstrapKind::Recovery, &req.token, ip).await?;
    let db = state.db.clone();
    let lookup = email.clone();
    let (user, revoked) = run_blocking(move || {
        let revoked = users::set_password(&db, &lookup, &hash_password(&req.password)?, false)?;
        let user = users::find_credentials(&db, &lookup)?
            .map(|c| c.user)
            .ok_or(UserError::NotFound(lookup))?;
        Ok((user, revoked))
    })
    .await?;
    state.sessions.revoke(&revoked);
    state.bootstrap.disarm();
    tracing::warn!(email = %user.email, "dashboard access recovered with the recovery token");
    audit::record(
        &state,
        AuditKind::RecoveryUsed,
        None,
        Some(&user.email),
        ip,
        None,
    )
    .await;
    let origin = session_origin(&headers, ip, true);
    let cookie = start_session(&state, &user.email, is_secure(&state, &headers), origin).await?;
    Ok(with_cookie(cookie, Json(user)))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    /// Email address or username.
    pub login: String,
    pub password: String,
    /// Keep the session across browser restarts. Defaults to true.
    #[serde(default)]
    pub remember: Option<bool>,
}

pub async fn login(
    State(state): State<AppState>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> ApiResult<Response> {
    if let Err(throttled) = state.login_throttle.check(&req.login, ip) {
        audit::record(
            &state,
            AuditKind::LoginThrottled,
            Some(&req.login),
            None,
            ip,
            None,
        )
        .await;
        return Err(throttled.into());
    }
    let db = state.db.clone();
    let (login, password) = (req.login.clone(), req.password);
    let user = run_blocking(move || {
        let found = users::find_credentials(&db, login.trim())?;
        // Verify against a dummy hash for unknown users so timing does not
        // reveal which accounts exist.
        let stored = found
            .as_ref()
            .map_or(dummy_hash(), |c| c.password_hash.as_str());
        let ok = verify_password(&password, stored);
        Ok(found.filter(|_| ok).map(|c| c.user))
    })
    .await?;
    let Some(user) = user else {
        state.login_throttle.record_failure(&req.login, ip);
        tracing::warn!(login = %req.login, ?ip, "failed dashboard login");
        audit::record(
            &state,
            AuditKind::LoginFailed,
            Some(&req.login),
            None,
            ip,
            None,
        )
        .await;
        return Err(InvalidCredentials.into());
    };
    state.login_throttle.clear_user(&req.login);
    audit::record(
        &state,
        AuditKind::LoginSucceeded,
        Some(&user.email),
        None,
        ip,
        None,
    )
    .await;
    let origin = session_origin(&headers, ip, req.remember.unwrap_or(true));
    let cookie = start_session(&state, &user.email, is_secure(&state, &headers), origin).await?;
    Ok(with_cookie(cookie, Json(user)))
}

/// A real hash of a throwaway password, so unknown accounts cost the same
/// as known ones.
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password(&random_token()).unwrap_or_default())
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token_hash) = token_hash_of(&headers) {
        let db = state.db.clone();
        let hash = token_hash.clone();
        run_blocking(move || users::delete_session(&db, &hash)).await?;
        state.sessions.revoke(&[token_hash]);
    }
    Ok(with_cookie(
        clear_cookie(is_secure(&state, &headers)),
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
    Extension(ClientIp(ip)): Extension<ClientIp>,
    headers: HeaderMap,
    Json(req): Json<ChangePassword>,
) -> ApiResult<Response> {
    validate_password(&req.new_password)?;
    if req.new_password == req.current_password {
        return Err(BadRequest("the new password must differ from the current one".into()).into());
    }
    let (db, email) = (state.db.clone(), auth.user.email.clone());
    let revoked = run_blocking(move || {
        let stored = users::password_hash(&db, &email)?.unwrap_or_default();
        if !verify_password(&req.current_password, &stored) {
            return Err(InvalidCredentials.into());
        }
        // Revokes every session, including this one; a fresh one follows.
        users::set_password(&db, &email, &hash_password(&req.new_password)?, false)
    })
    .await?;
    state.sessions.revoke(&revoked);
    let actor = Some(auth.user.email.as_str());
    audit::record(&state, AuditKind::PasswordChanged, actor, None, ip, None).await;
    let origin = session_origin(&headers, ip, auth.session.remember);
    let cookie = start_session(
        &state,
        &auth.user.email,
        is_secure(&state, &headers),
        origin,
    )
    .await?;
    Ok(with_cookie(cookie, StatusCode::NO_CONTENT))
}

#[derive(Serialize)]
pub struct SessionView {
    #[serde(flatten)]
    pub session: SessionInfo,
    pub current: bool,
}

pub async fn list_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> ApiResult<Json<Vec<SessionView>>> {
    let (db, email) = (state.db.clone(), auth.user.email.clone());
    let sessions = run_blocking(move || users::list_sessions(&db, &email)).await?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|session| SessionView {
                current: session.id == auth.session.id,
                session,
            })
            .collect(),
    ))
}

/// Ends one of the caller's own sessions; ending the current one signs out.
pub async fn revoke_session(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let (db, email, lookup) = (state.db.clone(), auth.user.email.clone(), id.clone());
    let hash = run_blocking(move || users::delete_session_by_id(&db, &email, &lookup)).await?;
    let Some(hash) = hash else {
        return Err(UserError::NotFound(id).into());
    };
    state.sessions.revoke(&[hash]);
    let actor = Some(auth.user.email.as_str());
    audit::record(
        &state,
        AuditKind::SessionRevoked,
        actor,
        None,
        ip,
        Some(&id),
    )
    .await;
    if id == auth.session.id {
        return Ok(with_cookie(
            clear_cookie(is_secure(&state, &headers)),
            StatusCode::NO_CONTENT,
        ));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn revoke_other_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
) -> ApiResult<StatusCode> {
    let (db, email, keep) = (
        state.db.clone(),
        auth.user.email.clone(),
        auth.session.token_hash.clone(),
    );
    let revoked = run_blocking(move || users::delete_other_sessions(&db, &email, &keep)).await?;
    state.sessions.revoke(&revoked);
    let (actor, detail) = (Some(auth.user.email.as_str()), revoked.len().to_string());
    audit::record(
        &state,
        AuditKind::OtherSessionsRevoked,
        actor,
        None,
        ip,
        Some(&detail),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AuditQuery {
    limit: Option<usize>,
}

/// The newest security events, for administrators.
pub async fn audit_log(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<Vec<AuthEvent>>> {
    auth.require_admin()?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let db = state.db.clone();
    Ok(Json(
        run_blocking(move || auth_events::recent(&db, limit)).await?,
    ))
}
