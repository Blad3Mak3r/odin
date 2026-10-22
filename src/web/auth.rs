//! Session authentication for the dashboard API: password hashing, the
//! session cookie, login throttling, and the middleware that guards `/api`.
//!
//! Sessions are opaque random tokens kept in an `HttpOnly; SameSite=Strict`
//! cookie; the database stores only their SHA-256. Viewers are read-only:
//! any method other than GET/HEAD requires an administrator.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use argon2::password_hash::PasswordVerifier;
use argon2::{Argon2, PasswordHasher};
use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rand::RngExt;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::db::users::{self, Role, SESSION_LIFETIME_DAYS, User};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

pub const SESSION_COOKIE: &str = "odin_session";
const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 128;
const MAX_FAILED_LOGINS: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);

/// The authenticated caller, inserted into request extensions by
/// [`require_auth`].
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
}

impl AuthUser {
    pub fn require_admin(&self) -> Result<(), crate::web::error::ApiError> {
        if self.user.role == Role::Admin {
            Ok(())
        } else {
            Err(Forbidden.into())
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("this action requires an administrator")]
pub struct Forbidden;

#[derive(Debug, thiserror::Error)]
#[error("invalid username or password")]
pub struct InvalidCredentials;

#[derive(Debug, thiserror::Error)]
#[error("too many failed attempts; try again in a minute")]
pub struct TooManyAttempts;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| anyhow::anyhow!("failed to hash password: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    Argon2::default()
        .verify_password(password.as_bytes(), stored)
        .is_ok()
}

pub fn validate_username(username: &str) -> Result<(), BadRequest> {
    let ok = (3..=32).contains(&username.len())
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(BadRequest(
            "username must be 3-32 characters of letters, digits, '.', '_' or '-'".into(),
        ))
    }
}

pub fn validate_password(password: &str) -> Result<(), BadRequest> {
    if (MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&password.chars().count()) {
        Ok(())
    } else {
        Err(BadRequest(format!(
            "password must be {MIN_PASSWORD_LEN}-{MAX_PASSWORD_LEN} characters"
        )))
    }
}

fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Creates a session for `user_id`, returning the cookie header value.
pub async fn start_session(state: &AppState, user_id: &str, secure: bool) -> ApiResult<String> {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let (db, user_id, token_hash) = (state.db.clone(), user_id.to_string(), hash_token(&token));
    run_blocking(move || users::create_session(&db, &user_id, &token_hash)).await?;
    Ok(cookie(&token, SESSION_LIFETIME_DAYS * 86_400, secure))
}

pub fn clear_cookie(secure: bool) -> String {
    cookie("", 0, secure)
}

fn cookie(value: &str, max_age: i64, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
    )
}

/// Whether the client reached us over HTTPS through a reverse proxy.
pub fn is_secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.to_string())
        .filter(|value| !value.is_empty())
}

/// Resolves the caller's session, if any.
pub async fn authenticate(state: &AppState, headers: &HeaderMap) -> ApiResult<Option<AuthUser>> {
    let Some(token) = session_token(headers) else {
        return Ok(None);
    };
    let (db, token_hash) = (state.db.clone(), hash_token(&token));
    let user = run_blocking(move || users::user_for_session(&db, &token_hash)).await?;
    Ok(user.map(|user| AuthUser { user }))
}

pub fn token_hash_of(headers: &HeaderMap) -> Option<String> {
    session_token(headers).map(|t| hash_token(&t))
}

/// Rejects cross-origin requests that carry an `Origin` different from `Host`.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let origin_host = origin
        .to_str()
        .ok()
        .and_then(|o| o.split_once("://"))
        .map(|(_, rest)| rest);
    matches!((host, origin_host), (Some(h), Some(o)) if h == o)
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// Paths under `/api` reachable without a session.
fn is_public(path: &str) -> bool {
    matches!(
        path,
        "/api/auth/status" | "/api/auth/login" | "/api/auth/setup"
    )
}

/// Paths a viewer may call with a mutating method.
fn viewer_may_mutate(path: &str) -> bool {
    matches!(path, "/api/auth/logout" | "/api/auth/password")
}

pub async fn require_auth(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    if !path.starts_with("/api/") && path != "/api" {
        return next.run(req).await;
    }
    let safe = matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if !safe && !same_origin(req.headers()) {
        return error_response(StatusCode::FORBIDDEN, "cross-origin request rejected");
    }
    if is_public(&path) {
        return next.run(req).await;
    }
    let auth = match authenticate(&state, req.headers()).await {
        Ok(Some(auth)) => auth,
        Ok(None) => return error_response(StatusCode::UNAUTHORIZED, "authentication required"),
        Err(err) => return err.into_response(),
    };
    if !safe && auth.user.role != Role::Admin && !viewer_may_mutate(&path) {
        return error_response(StatusCode::FORBIDDEN, &Forbidden.to_string());
    }
    req.extensions_mut().insert(auth);
    next.run(req).await
}

/// Per-username failed-login counter, held in memory.
#[derive(Clone, Default)]
pub struct LoginThrottle(Arc<Mutex<HashMap<String, (u32, Instant)>>>);

impl LoginThrottle {
    fn key(username: &str) -> String {
        username.to_ascii_lowercase()
    }

    pub fn check(&self, username: &str) -> Result<(), TooManyAttempts> {
        let mut map = self.0.lock().expect("login throttle lock poisoned");
        match map.get(&Self::key(username)) {
            Some((n, at)) if *n >= MAX_FAILED_LOGINS && at.elapsed() < LOCKOUT => {
                Err(TooManyAttempts)
            }
            Some((_, at)) if at.elapsed() >= LOCKOUT => {
                map.remove(&Self::key(username));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn record_failure(&self, username: &str) {
        let mut map = self.0.lock().expect("login throttle lock poisoned");
        let entry = map
            .entry(Self::key(username))
            .or_insert((0, Instant::now()));
        entry.0 += 1;
        entry.1 = Instant::now();
    }

    pub fn clear(&self, username: &str) {
        self.0
            .lock()
            .expect("login throttle lock poisoned")
            .remove(&Self::key(username));
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;
    use crate::db::Db;
    use crate::paths::Paths;

    fn app() -> Router {
        let dir = std::env::temp_dir().join(format!(
            "odin-auth-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        crate::web::router::build_app(AppState::new(paths, db))
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, Option<String>, serde_json::Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(cookie) = cookie {
            req = req.header(header::COOKIE, cookie);
        }
        let body = match body {
            Some(json) => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        let response = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let set_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_string);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, set_cookie, json)
    }

    fn creds(username: &str, password: &str) -> Option<serde_json::Value> {
        Some(json!({ "username": username, "password": password }))
    }

    #[tokio::test]
    async fn api_requires_session_and_setup_runs_once() {
        let app = app();
        let (status, ..) = call(&app, "GET", "/api/webhooks", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (_, _, body) = call(&app, "GET", "/api/auth/status", None, None).await;
        assert_eq!(body["setup_required"], true);

        let (status, cookie, _) = call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("admin", "correct horse"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let cookie = cookie.unwrap();

        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::OK);

        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("evil", "correct horse"),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn login_rejects_bad_password_and_logout_revokes_session() {
        let app = app();
        call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("admin", "correct horse"),
        )
        .await;

        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            creds("admin", "wrong password"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, cookie, _) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            creds("ADMIN", "correct horse"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let cookie = cookie.unwrap();

        let (status, ..) = call(&app, "POST", "/api/auth/logout", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&cookie), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn viewers_are_read_only_and_last_admin_is_protected() {
        let app = app();
        let (_, admin, admin_user) = call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("admin", "correct horse"),
        )
        .await;
        let admin = admin.unwrap();

        let new_user =
            Some(json!({ "username": "watcher", "password": "long enough", "role": "viewer" }));
        let (status, ..) = call(&app, "POST", "/api/users", Some(&admin), new_user).await;
        assert_eq!(status, StatusCode::OK);

        let (_, viewer, _) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            creds("watcher", "long enough"),
        )
        .await;
        let viewer = viewer.unwrap();
        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&viewer), None).await;
        assert_eq!(status, StatusCode::OK);
        let hook = Some(json!({ "url": "https://example.com/hook" }));
        let (status, ..) = call(&app, "POST", "/api/webhooks", Some(&viewer), hook).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, ..) = call(&app, "GET", "/api/users", Some(&viewer), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let id = admin_user["id"].as_str().unwrap();
        let (status, ..) = call(
            &app,
            "DELETE",
            &format!("/api/users/{id}"),
            Some(&admin),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn cross_origin_mutations_are_rejected() {
        let app = app();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header(header::HOST, "odin.local:7331")
                    .header(header::ORIGIN, "http://evil.example")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "username": "a", "password": "b" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn throttle_locks_after_repeated_failures() {
        let throttle = LoginThrottle::default();
        for _ in 0..MAX_FAILED_LOGINS {
            assert!(throttle.check("Admin").is_ok());
            throttle.record_failure("admin");
        }
        assert!(throttle.check("ADMIN").is_err());
        throttle.clear("admin");
        assert!(throttle.check("admin").is_ok());
    }
}
