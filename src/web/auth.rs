//! Session authentication and request hardening for the dashboard API:
//! password hashing, the session cookie, CSRF defences, login throttling,
//! security response headers, and the middleware that guards `/api`.
//!
//! Sessions are opaque random tokens kept in an `HttpOnly; SameSite=Strict`
//! cookie; the database stores only their SHA-256. State-changing requests
//! need a same-origin `Origin` and a per-session `X-CSRF-Token`. Viewers are
//! read-only: any method other than GET/HEAD requires an administrator.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use argon2::password_hash::PasswordVerifier;
use argon2::{Argon2, PasswordHasher};
use axum::Json;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rand::RngExt;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::db::users::{self, Role, SESSION_LIFETIME_DAYS, User};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

pub const SESSION_COOKIE: &str = "odin_session";
/// Used instead of [`SESSION_COOKIE`] over HTTPS: the `__Host-` prefix makes
/// browsers refuse the cookie unless it is `Secure`, host-only and `Path=/`.
pub const SECURE_SESSION_COOKIE: &str = "__Host-odin_session";
pub const CSRF_HEADER: &str = "x-csrf-token";
const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 128;
const LOCKOUT: Duration = Duration::from_secs(60);
const MAX_FAILURES_PER_USER: u32 = 5;
const MAX_FAILURES_PER_IP: u32 = 20;
/// Bound on tracked throttle keys, so spraying usernames cannot grow memory.
const MAX_TRACKED_KEYS: usize = 10_000;

const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
    style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https:; \
    font-src 'self' data:; connect-src 'self'; object-src 'none'; \
    base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

/// The authenticated caller, inserted into request extensions by
/// [`require_auth`].
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
    /// Token the client must echo in `X-CSRF-Token` on mutating requests.
    pub csrf_token: String,
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

/// A blank username means "none". Usernames cannot contain `@`, so a login
/// string is never ambiguous between an email and a username.
pub fn optional_username(raw: Option<&str>) -> Result<Option<String>, BadRequest> {
    match raw.map(str::trim).filter(|name| !name.is_empty()) {
        None => Ok(None),
        Some(name) => validate_username(name).map(|()| Some(name.to_string())),
    }
}

/// Checks an email address (RFC 5321 limits, dot-atom local part, DNS-style
/// domain with a dot) and returns it lowercased, the form stored and compared.
pub fn normalize_email(raw: &str) -> Result<String, BadRequest> {
    let invalid = || BadRequest("enter a valid email address".into());
    let email = raw.trim().to_ascii_lowercase();
    if email.len() > 254 {
        return Err(invalid());
    }
    let (local, domain) = email.split_once('@').ok_or_else(invalid)?;
    let local_ok = (1..=64).contains(&local.len())
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+/=?^_`{|}~.-".contains(c));
    let labels: Vec<&str> = domain.split('.').collect();
    let domain_ok = labels.len() >= 2
        && labels.iter().all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        && labels
            .last()
            .is_some_and(|tld| tld.len() >= 2 && tld.chars().any(|c| c.is_ascii_alphabetic()));
    if local_ok && domain_ok {
        Ok(email)
    } else {
        Err(invalid())
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// Derived from the session token, so it needs no storage and dies with the
/// session. Script on another origin can neither read nor guess it.
pub fn csrf_token_for(session_token: &str) -> String {
    hex(&Sha256::digest(
        format!("odin-csrf:{session_token}").as_bytes(),
    ))
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |diff, (x, y)| diff | (x ^ y))
            == 0
}

/// Creates a session for `user_id`, returning the cookie header value.
pub async fn start_session(state: &AppState, user_email: &str, secure: bool) -> ApiResult<String> {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let token = hex(&bytes);
    let (db, user_email, token_hash) =
        (state.db.clone(), user_email.to_string(), hash_token(&token));
    run_blocking(move || users::create_session(&db, &user_email, &token_hash)).await?;
    Ok(cookie(&token, SESSION_LIFETIME_DAYS * 86_400, secure))
}

pub fn clear_cookie(secure: bool) -> String {
    cookie("", 0, secure)
}

fn cookie(value: &str, max_age: i64, secure: bool) -> String {
    let (name, flag) = if secure {
        (SECURE_SESSION_COOKIE, "; Secure")
    } else {
        (SESSION_COOKIE, "")
    };
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{flag}")
}

/// Whether the browser reached us over HTTPS: the configured public URL says
/// so, or a reverse proxy reported it.
pub fn is_secure(state: &AppState, headers: &HeaderMap) -> bool {
    state.public_origin.as_ref().is_some_and(|o| o.https)
        || headers
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
        .find(|(name, _)| *name == SESSION_COOKIE || *name == SECURE_SESSION_COOKIE)
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
    Ok(user.map(|user| AuthUser {
        user,
        csrf_token: csrf_token_for(&token),
    }))
}

pub fn token_hash_of(headers: &HeaderMap) -> Option<String> {
    session_token(headers).map(|t| hash_token(&t))
}

/// Whether a state-changing request plausibly comes from our own pages.
/// Browsers always send `Origin` on cross-origin and on same-origin
/// non-GET requests, and `Sec-Fetch-Site` on every request; a request with
/// neither is not from a browser and cannot carry a victim's cookie.
fn origin_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    if let Some(origin) = headers.get(header::ORIGIN) {
        let Ok(origin) = origin.to_str().map(str::to_ascii_lowercase) else {
            return false;
        };
        if state
            .public_origin
            .as_ref()
            .is_some_and(|p| p.origin == origin)
        {
            return true;
        }
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(str::to_ascii_lowercase);
        return matches!(
            (host, origin.split_once("://")),
            (Some(h), Some((_, o))) if h == o
        );
    }
    headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|site| matches!(site, "same-origin" | "none"))
}

fn csrf_valid(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|given| constant_time_eq(given, expected))
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
    if !safe && !origin_allowed(&state, req.headers()) {
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
    if !safe && !csrf_valid(req.headers(), &auth.csrf_token) {
        return error_response(StatusCode::FORBIDDEN, "missing or invalid CSRF token");
    }
    if !safe && auth.user.role != Role::Admin && !viewer_may_mutate(&path) {
        return error_response(StatusCode::FORBIDDEN, &Forbidden.to_string());
    }
    req.extensions_mut().insert(auth);
    next.run(req).await
}

/// Adds browser-hardening headers to every response, and keeps API
/// responses out of shared and browser caches.
pub async fn security_headers(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let is_api = req.uri().path().starts_with("/api");
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    let fixed = [
        ("content-security-policy", CONTENT_SECURITY_POLICY),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "same-origin"),
        ("cross-origin-opener-policy", "same-origin"),
        ("cross-origin-resource-policy", "same-origin"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
        ),
    ];
    for (name, value) in fixed {
        headers.insert(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    if is_api {
        headers
            .entry(header::CACHE_CONTROL)
            .or_insert(HeaderValue::from_static("no-store"));
    }
    if state.public_origin.as_ref().is_some_and(|o| o.https) {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}

/// The client address for throttling: the TCP peer, or, when that is a local
/// reverse proxy, the last `X-Forwarded-For` hop it appended.
pub fn client_ip(headers: &HeaderMap, peer: Option<&ConnectInfo<SocketAddr>>) -> Option<IpAddr> {
    let peer = peer?.0.ip();
    if peer.is_loopback()
        && let Some(forwarded) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .and_then(|hop| hop.trim().parse::<IpAddr>().ok())
    {
        return Some(forwarded);
    }
    Some(peer)
}

/// Failed-login counters held in memory, keyed by username and by client
/// address so neither one hammered account nor one noisy client is free.
#[derive(Clone, Default)]
pub struct LoginThrottle(Arc<Mutex<HashMap<String, (u32, Instant)>>>);

impl LoginThrottle {
    fn keys(username: &str, ip: Option<IpAddr>) -> [(String, u32); 2] {
        [
            (
                format!("user:{}", username.to_ascii_lowercase()),
                MAX_FAILURES_PER_USER,
            ),
            (
                format!("ip:{}", ip.map(|ip| ip.to_string()).unwrap_or_default()),
                MAX_FAILURES_PER_IP,
            ),
        ]
    }

    pub fn check(&self, username: &str, ip: Option<IpAddr>) -> Result<(), TooManyAttempts> {
        let mut map = self.0.lock().expect("login throttle lock poisoned");
        for (key, max) in Self::keys(username, ip) {
            match map.get(&key) {
                Some((_, at)) if at.elapsed() >= LOCKOUT => {
                    map.remove(&key);
                }
                Some((n, _)) if *n >= max => return Err(TooManyAttempts),
                _ => {}
            }
        }
        Ok(())
    }

    pub fn record_failure(&self, username: &str, ip: Option<IpAddr>) {
        let mut map = self.0.lock().expect("login throttle lock poisoned");
        if map.len() >= MAX_TRACKED_KEYS {
            map.retain(|_, (_, at)| at.elapsed() < LOCKOUT);
        }
        for (key, _) in Self::keys(username, ip) {
            if map.len() >= MAX_TRACKED_KEYS && !map.contains_key(&key) {
                continue;
            }
            let entry = map.entry(key).or_insert((0, Instant::now()));
            entry.0 += 1;
            entry.1 = Instant::now();
        }
    }

    /// Forgives the account's counter after a successful login; the
    /// address counter is left to expire so a valid account cannot be used
    /// to reset it.
    pub fn clear_user(&self, username: &str) {
        self.0
            .lock()
            .expect("login throttle lock poisoned")
            .remove(&Self::keys(username, None)[0].0);
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
        app_with(None)
    }

    fn app_with(public: Option<&str>) -> Router {
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
        let public = public.map(|raw| crate::config::PublicOrigin::parse(raw).unwrap());
        crate::web::router::build_app(AppState::new(paths, db).with_public_origin(public))
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
            let token = cookie.split_once('=').unwrap().1;
            req = req.header(CSRF_HEADER, csrf_token_for(token));
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
        Some(json!({
            "email": format!("{username}@example.com"),
            "username": username,
            "password": password,
        }))
    }

    fn login_creds(login: &str, password: &str) -> Option<serde_json::Value> {
        Some(json!({ "login": login, "password": password }))
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
            login_creds("admin", "wrong password"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, cookie, _) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("ADMIN", "correct horse"),
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

        let new_user = Some(
            json!({ "email": "watcher@example.com", "username": "watcher", "password": "long enough", "role": "viewer" }),
        );
        let (status, ..) = call(&app, "POST", "/api/users", Some(&admin), new_user).await;
        assert_eq!(status, StatusCode::OK);

        let (_, viewer, _) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("watcher", "long enough"),
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

        let id = admin_user["email"].as_str().unwrap();
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
                        json!({ "login": "a", "password": "b" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn email_is_the_key_and_username_is_optional_but_unique() {
        let app = app();
        let setup = Some(json!({ "email": "Admin@Example.com", "password": "correct horse" }));
        let (status, admin, body) = call(&app, "POST", "/api/auth/setup", None, setup).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["email"], "admin@example.com");
        assert!(body["username"].is_null());
        let admin = admin.unwrap();

        // Login works by email (any case) even without a username.
        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("ADMIN@example.com", "correct horse"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let new = |email: &str, username: Option<&str>| {
            Some(json!({
                "email": email,
                "username": username,
                "password": "long enough",
                "role": "viewer",
            }))
        };
        let add = |email: &'static str, username: Option<&'static str>| {
            let (app, admin) = (app.clone(), admin.clone());
            async move {
                call(
                    &app,
                    "POST",
                    "/api/users",
                    Some(&admin),
                    new(email, username),
                )
                .await
                .0
            }
        };
        assert_eq!(add("a@example.com", Some("alice")).await, StatusCode::OK);
        assert_eq!(add("b@example.com", None).await, StatusCode::OK);
        assert_eq!(add("c@example.com", None).await, StatusCode::OK);
        assert_eq!(
            add("A@example.com", Some("bob")).await,
            StatusCode::CONFLICT
        );
        assert_eq!(
            add("d@example.com", Some("ALICE")).await,
            StatusCode::CONFLICT
        );
        assert_eq!(add("not-an-email", None).await, StatusCode::BAD_REQUEST);
        assert_eq!(add("x@localhost", None).await, StatusCode::BAD_REQUEST);
        assert_eq!(
            add("e@example.com", Some("a@b")).await,
            StatusCode::BAD_REQUEST
        );

        // The username logs in too.
        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("Alice", "long enough"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[test]
    fn email_validation_normalizes_and_rejects_malformed_addresses() {
        assert_eq!(
            normalize_email(" Me.You+tag@Mail.Example.COM ").unwrap(),
            "me.you+tag@mail.example.com"
        );
        for bad in [
            "",
            "plain",
            "@example.com",
            "me@",
            "me@@example.com",
            "me@example",
            "me@-a.com",
            "me@a..com",
            ".me@example.com",
            "me..you@example.com",
            "me you@example.com",
            "me@example.c",
            "me@123.456",
        ] {
            assert!(normalize_email(bad).is_err(), "{bad}");
        }
        assert!(normalize_email(&format!("{}@example.com", "a".repeat(65))).is_err());
    }

    async fn raw(app: &Router, req: Request<Body>) -> Response {
        app.clone().oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn mutations_need_the_session_csrf_token() {
        let app = app();
        let (_, cookie, _) = call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("admin", "correct horse"),
        )
        .await;
        let cookie = cookie.unwrap();
        let token = cookie.split_once('=').unwrap().1;

        let (_, _, status) = call(&app, "GET", "/api/auth/status", Some(&cookie), None).await;
        assert_eq!(status["csrf_token"], csrf_token_for(token));

        let post = |csrf: Option<&str>| {
            let mut req = Request::builder()
                .method("POST")
                .uri("/api/auth/logout")
                .header(header::COOKIE, &cookie);
            if let Some(csrf) = csrf {
                req = req.header(CSRF_HEADER, csrf);
            }
            req.body(Body::empty()).unwrap()
        };
        assert_eq!(raw(&app, post(None)).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            raw(&app, post(Some("wrong"))).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            raw(&app, post(Some(&csrf_token_for(token)))).await.status(),
            StatusCode::NO_CONTENT
        );
    }

    #[tokio::test]
    async fn browser_fetch_metadata_must_be_same_origin() {
        let app = app();
        let login = |site: &str| {
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("sec-fetch-site", site)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "login": "a", "password": "b" }).to_string(),
                ))
                .unwrap()
        };
        assert_eq!(
            raw(&app, login("cross-site")).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            raw(&app, login("same-site")).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_ne!(
            raw(&app, login("same-origin")).await.status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn public_url_enables_proxied_origin_secure_cookie_and_hsts() {
        let app = app_with(Some("https://odin.example.com"));
        let setup = |origin: &str| {
            Request::builder()
                .method("POST")
                .uri("/api/auth/setup")
                .header(header::HOST, "127.0.0.1:7331")
                .header(header::ORIGIN, origin)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "email": "admin@example.com", "username": "admin", "password": "correct horse" }).to_string(),
                ))
                .unwrap()
        };
        assert_eq!(
            raw(&app, setup("https://evil.example")).await.status(),
            StatusCode::FORBIDDEN
        );
        let response = raw(&app, setup("https://odin.example.com")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(set_cookie.starts_with("__Host-odin_session="));
        assert!(set_cookie.contains("; Secure"));
        assert!(
            response
                .headers()
                .contains_key(header::STRICT_TRANSPORT_SECURITY)
        );
    }

    #[tokio::test]
    async fn responses_carry_security_headers() {
        let app = app();
        let response = raw(
            &app,
            Request::builder()
                .uri("/api/auth/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let headers = response.headers();
        assert!(
            headers["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );
        assert_eq!(headers["x-frame-options"], "DENY");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert!(!headers.contains_key(header::STRICT_TRANSPORT_SECURITY));
    }

    #[test]
    fn throttle_locks_by_username_and_by_address() {
        let throttle = LoginThrottle::default();
        let ip: Option<IpAddr> = Some("203.0.113.9".parse().unwrap());
        for _ in 0..MAX_FAILURES_PER_USER {
            assert!(throttle.check("Admin", ip).is_ok());
            throttle.record_failure("admin", ip);
        }
        assert!(throttle.check("ADMIN", ip).is_err());
        throttle.clear_user("admin");
        assert!(throttle.check("admin", ip).is_ok());

        for n in 0..MAX_FAILURES_PER_IP {
            throttle.record_failure(&format!("user{n}"), ip);
        }
        assert!(throttle.check("someone-new", ip).is_err());
        assert!(throttle.check("someone-new", None).is_ok());
    }

    #[test]
    fn client_ip_trusts_forwarded_for_only_from_a_local_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "198.51.100.1, 203.0.113.7".parse().unwrap(),
        );
        let local = ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap());
        let remote = ConnectInfo("192.0.2.5:5000".parse::<SocketAddr>().unwrap());
        assert_eq!(
            client_ip(&headers, Some(&local)),
            "203.0.113.7".parse().ok()
        );
        assert_eq!(client_ip(&headers, Some(&remote)), "192.0.2.5".parse().ok());
        assert_eq!(client_ip(&headers, None), None);
    }
}
