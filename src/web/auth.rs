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
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use rand::RngExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use crate::config::TrustedProxies;
use crate::db::users::{self, Role, SessionInfo, SessionOrigin, User};
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
    pub session: SessionInfo,
    /// The session's expiry just slid forward, so the cookie should too.
    pub refreshed: bool,
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

#[derive(Debug, thiserror::Error)]
#[error("invalid or missing token")]
pub struct InvalidBootstrapToken;

/// What a one-time token printed in the server log unlocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapKind {
    /// Creating the first administrator.
    Setup,
    /// Resetting a password after losing access.
    Recovery,
}

/// Holds the one-time token that gates first-run setup or recovery. It lives
/// only in memory and in the server log, so using it proves access to the
/// host, not just to the network port.
#[derive(Clone, Default)]
pub struct BootstrapGate(Arc<Mutex<Option<(BootstrapKind, String)>>>);

impl BootstrapGate {
    pub fn arm(&self, kind: BootstrapKind, token: String) {
        *self.0.lock().expect("bootstrap gate lock poisoned") = Some((kind, token));
    }

    pub fn kind(&self) -> Option<BootstrapKind> {
        self.0
            .lock()
            .expect("bootstrap gate lock poisoned")
            .as_ref()
            .map(|(kind, _)| *kind)
    }

    pub fn matches(&self, kind: BootstrapKind, given: &str) -> bool {
        self.0
            .lock()
            .expect("bootstrap gate lock poisoned")
            .as_ref()
            .is_some_and(|(k, token)| *k == kind && constant_time_eq(token, given.trim()))
    }

    pub fn disarm(&self) {
        *self.0.lock().expect("bootstrap gate lock poisoned") = None;
    }
}

/// Marker file that, when present at startup, arms a recovery token.
pub const RECOVERY_MARKER: &str = "recover-admin";

/// Arms the setup token when no account exists, or the recovery token when
/// the operator left a [`RECOVERY_MARKER`] file in the data directory, and
/// prints it. The marker is consumed so recovery needs a deliberate restart.
pub fn arm_bootstrap(state: &AppState) -> anyhow::Result<()> {
    let marker = state.paths.data_dir.join(RECOVERY_MARKER);
    let marked = marker.exists();
    if marked {
        std::fs::remove_file(&marker)?;
    }
    if users::count(&state.db)? == 0 {
        let token = random_token();
        println!("Dashboard setup token (needed to create the first account): {token}");
        tracing::warn!(%token, "dashboard has no accounts; setup token issued");
        state.bootstrap.arm(BootstrapKind::Setup, token);
    } else if marked {
        let token = random_token();
        println!("Dashboard recovery token (valid until used or restart): {token}");
        tracing::warn!(%token, "dashboard recovery requested; recovery token issued");
        state.bootstrap.arm(BootstrapKind::Recovery, token);
    }
    Ok(())
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill(&mut bytes);
    hex(&bytes)
}

/// Live event streams, keyed by session token hash, so revoking a session
/// ends the streams it opened.
#[derive(Clone, Default)]
pub struct SessionRegistry(Arc<Mutex<HashMap<String, watch::Sender<bool>>>>);

impl SessionRegistry {
    fn watch(&self, token_hash: &str) -> watch::Receiver<bool> {
        let mut map = self.0.lock().expect("session registry lock poisoned");
        map.retain(|_, tx| tx.receiver_count() > 0);
        map.entry(token_hash.to_string())
            .or_insert_with(|| watch::channel(false).0)
            .subscribe()
    }

    /// Signals every stream of the given sessions to end.
    pub fn revoke(&self, token_hashes: &[String]) {
        let mut map = self.0.lock().expect("session registry lock poisoned");
        for hash in token_hashes {
            if let Some(tx) = map.remove(hash) {
                tx.send_replace(true);
            }
        }
    }
}

/// Describes the client for the session list.
pub fn session_origin(headers: &HeaderMap, ip: Option<IpAddr>, remember: bool) -> SessionOrigin {
    SessionOrigin {
        remember,
        ip: ip.map(|ip| ip.to_string()),
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|ua| ua.chars().filter(|c| !c.is_control()).take(200).collect()),
    }
}

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

/// Creates a session for `user_email`, returning the cookie header value.
pub async fn start_session(
    state: &AppState,
    user_email: &str,
    secure: bool,
    origin: SessionOrigin,
) -> ApiResult<String> {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let token = hex(&bytes);
    let (db, user_email, token_hash) =
        (state.db.clone(), user_email.to_string(), hash_token(&token));
    let remember = origin.remember;
    run_blocking(move || users::create_session(&db, &user_email, &token_hash, &origin)).await?;
    Ok(session_cookie(&token, remember, secure))
}

/// A remembered session's cookie outlives the browser; any other dies with it.
fn session_cookie(token: &str, remember: bool, secure: bool) -> String {
    let max_age = remember.then_some(users::REMEMBER_IDLE_DAYS * 86_400);
    cookie(token, max_age, secure)
}

pub fn clear_cookie(secure: bool) -> String {
    cookie("", Some(0), secure)
}

fn cookie(value: &str, max_age: Option<i64>, secure: bool) -> String {
    let (name, flag) = if secure {
        (SECURE_SESSION_COOKIE, "; Secure")
    } else {
        (SESSION_COOKIE, "")
    };
    let max_age = max_age.map_or(String::new(), |age| format!("; Max-Age={age}"));
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Strict{max_age}{flag}")
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

/// Resolves the caller's session, if any, sliding its expiry forward.
pub async fn authenticate(state: &AppState, headers: &HeaderMap) -> ApiResult<Option<AuthUser>> {
    let Some(token) = session_token(headers) else {
        return Ok(None);
    };
    let (db, token_hash) = (state.db.clone(), hash_token(&token));
    let found = run_blocking(move || {
        let Some((user, session)) = users::user_for_session(&db, &token_hash)? else {
            return Ok(None);
        };
        let refreshed = users::touch_session(&db, &session)?;
        Ok(Some((user, session, refreshed)))
    })
    .await?;
    Ok(found.map(|(user, session, refreshed)| AuthUser {
        user,
        csrf_token: csrf_token_for(&token),
        session,
        refreshed,
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
        "/api/auth/status" | "/api/auth/login" | "/api/auth/setup" | "/api/auth/recover"
    )
}

/// What an account whose password someone else chose may still do: replace
/// it, or leave. The status route is public and tells the dashboard why.
fn allowed_before_password_change(path: &str) -> bool {
    matches!(path, "/api/auth/password" | "/api/auth/logout")
}

/// Paths a viewer may call with a mutating method: their own account only.
fn viewer_may_mutate(path: &str) -> bool {
    matches!(path, "/api/auth/logout" | "/api/auth/password")
        || path.starts_with("/api/auth/sessions")
}

pub async fn require_auth(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let headers = req.headers().clone();
    if !path.starts_with("/api/") && path != "/api" {
        return next.run(req).await;
    }
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip());
    let ip = resolve_client_ip(&state.trusted_proxies, &headers, peer);
    req.extensions_mut().insert(ClientIp(ip));
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
    if auth.user.must_change_password && !allowed_before_password_change(&path) {
        return error_response(StatusCode::FORBIDDEN, "password change required");
    }
    if !safe && auth.user.role != Role::Admin && !viewer_may_mutate(&path) {
        return error_response(StatusCode::FORBIDDEN, &Forbidden.to_string());
    }
    let token = session_token(req.headers());
    req.extensions_mut().insert(auth.clone());
    let mut response = next.run(req).await;
    if auth.refreshed
        && auth.session.remember
        && !response.headers().contains_key(header::SET_COOKIE)
        && let Some(token) = token
        && let Ok(value) =
            HeaderValue::from_str(&session_cookie(&token, true, is_secure(&state, &headers)))
    {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    end_stream_on_revoke(&state, &auth, response).await
}

/// Server-sent event streams outlive the request that opened them, so they
/// are cut when their session is revoked or runs out.
async fn end_stream_on_revoke(state: &AppState, auth: &AuthUser, response: Response) -> Response {
    let is_stream = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    if !is_stream {
        return response;
    }
    let mut revoked = state.sessions.watch(&auth.session.token_hash);
    // A revocation between authenticating and subscribing would be missed.
    let (db, hash) = (state.db.clone(), auth.session.token_hash.clone());
    if let Ok(None) = run_blocking(move || users::user_for_session(&db, &hash)).await {
        return error_response(StatusCode::UNAUTHORIZED, "authentication required");
    }
    let lifetime = (auth.session.expires_at - chrono::Utc::now())
        .to_std()
        .unwrap_or_default();
    let stop = async move {
        tokio::select! {
            _ = revoked.wait_for(|revoked| *revoked) => {}
            _ = tokio::time::sleep(lifetime) => {}
        }
    };
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        Body::from_stream(body.into_data_stream().take_until(stop)),
    )
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

/// The caller's address as resolved by [`require_auth`]; `None` without a socket.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub Option<IpAddr>);

/// The client address for throttling and the audit log. A connection from a
/// trusted proxy is believed about who is behind it: the nearest
/// `X-Forwarded-For` hop that is not itself a trusted proxy. Anyone else's
/// header is ignored, so it cannot be used to dodge the limits.
pub fn resolve_client_ip(
    trusted: &TrustedProxies,
    headers: &HeaderMap,
    peer: Option<IpAddr>,
) -> Option<IpAddr> {
    let peer = peer?.to_canonical();
    if !trusted.contains(peer) {
        return Some(peer);
    }
    let hops: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .collect();
    for hop in hops.into_iter().rev() {
        match hop.trim().parse::<IpAddr>() {
            Ok(ip) if trusted.contains(ip) => continue,
            Ok(ip) => return Some(ip.to_canonical()),
            Err(_) => break,
        }
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
        app_state(public).0
    }

    /// The router plus its state, with a random setup token armed.
    fn app_state(public: Option<&str>) -> (Router, AppState) {
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
        let state = AppState::new(paths, db).with_public_origin(public);
        state.bootstrap.arm(BootstrapKind::Setup, setup_token());
        (crate::web::router::build_app(state.clone()), state)
    }

    /// Credentials are generated for the test run and never written in
    /// source: one random password per label, stable within the process.
    fn pw(label: &str) -> String {
        static PASSWORDS: std::sync::OnceLock<Mutex<HashMap<String, String>>> =
            std::sync::OnceLock::new();
        PASSWORDS
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .entry(label.to_string())
            .or_insert_with(random_token)
            .clone()
    }

    fn setup_token() -> String {
        pw("setup-token")
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
            "token": setup_token(),
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
            creds("admin", &pw("admin")),
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
            creds("evil", &pw("admin")),
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
            creds("admin", &pw("admin")),
        )
        .await;

        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("admin", &pw("wrong")),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, cookie, _) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("ADMIN", &pw("admin")),
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
            creds("admin", &pw("admin")),
        )
        .await;
        let admin = admin.unwrap();

        let new_user = Some(
            json!({ "email": "watcher@example.com", "username": "watcher", "password": pw("user"), "role": "viewer", "require_change": false }),
        );
        let (status, ..) = call(&app, "POST", "/api/users", Some(&admin), new_user).await;
        assert_eq!(status, StatusCode::OK);

        let (_, viewer, _) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("watcher", &pw("user")),
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
                        json!({ "login": "a", "password": pw("wrong") }).to_string(),
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
        let setup = Some(
            json!({ "token": setup_token(), "email": "Admin@Example.com", "password": pw("admin") }),
        );
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
            login_creds("ADMIN@example.com", &pw("admin")),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let new = |email: &str, username: Option<&str>| {
            Some(json!({
                "email": email,
                "username": username,
                "password": pw("user"),
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
            login_creds("Alice", &pw("user")),
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

    #[tokio::test]
    async fn setup_needs_the_logged_token_and_recovery_needs_the_marker() {
        let (app, state) = app_state(None);
        let body = |token: &str| {
            Some(json!({ "token": token, "email": "a@example.com", "password": pw("admin") }))
        };
        for wrong in [String::new(), pw("wrong")] {
            let (status, ..) = call(&app, "POST", "/api/auth/setup", None, body(&wrong)).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{wrong:?}");
        }
        let (_, _, status) = call(&app, "GET", "/api/auth/status", None, None).await;
        assert_eq!(status["setup_required"], true);
        let (status, ..) = call(&app, "POST", "/api/auth/setup", None, body(&setup_token())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(state.bootstrap.kind(), None);

        // No marker file: restart arms nothing, so recovery is refused.
        arm_bootstrap(&state).unwrap();
        assert_eq!(state.bootstrap.kind(), None);
        let recover = |token: &str, email: &str| {
            Some(json!({ "token": token, "email": email, "password": pw("recovered") }))
        };
        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/recover",
            None,
            recover(&pw("wrong"), "a@example.com"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // The marker arms a one-time token and is consumed.
        let marker = state.paths.data_dir.join(RECOVERY_MARKER);
        std::fs::write(&marker, "").unwrap();
        arm_bootstrap(&state).unwrap();
        assert!(!marker.exists());
        assert_eq!(state.bootstrap.kind(), Some(BootstrapKind::Recovery));
        let (_, _, status) = call(&app, "GET", "/api/auth/status", None, None).await;
        assert_eq!(status["recovery_active"], true);

        let token = state.bootstrap.0.lock().unwrap().clone().unwrap().1;
        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/recover",
            None,
            recover(&token, "ghost@example.com"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, cookie, _) = call(
            &app,
            "POST",
            "/api/auth/recover",
            None,
            recover(&token, "A@example.com"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(cookie.is_some());
        assert_eq!(state.bootstrap.kind(), None);

        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("a@example.com", &pw("admin")),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("a@example.com", &pw("recovered")),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    async fn login_cookie(app: &Router, login: &str, password: &str, remember: bool) -> String {
        let body = json!({ "login": login, "password": password, "remember": remember });
        let response = raw(
            app,
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::USER_AGENT, "Mozilla/5.0 test")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert_eq!(set_cookie.contains("Max-Age="), remember, "{set_cookie}");
        set_cookie.split(';').next().unwrap().to_string()
    }

    #[tokio::test]
    async fn sessions_can_be_listed_and_revoked_by_their_owner() {
        let app = app();
        call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("admin", &pw("admin")),
        )
        .await;
        let first = login_cookie(&app, "admin", &pw("admin"), true).await;
        let second = login_cookie(&app, "admin", &pw("admin"), false).await;

        let (_, _, list) = call(&app, "GET", "/api/auth/sessions", Some(&first), None).await;
        let list = list.as_array().unwrap();
        assert_eq!(list.len(), 3); // setup + two logins
        assert_eq!(list.iter().filter(|s| s["current"] == true).count(), 1);
        assert!(list.iter().all(|s| s.get("token_hash").is_none()));
        assert!(list.iter().any(|s| s["user_agent"] == "Mozilla/5.0 test"));
        assert!(list.iter().any(|s| s["remember"] == false));

        let other = list.iter().find(|s| s["current"] == false).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let (status, ..) = call(
            &app,
            "DELETE",
            &format!("/api/auth/sessions/{other}"),
            Some(&first),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, ..) = call(
            &app,
            "DELETE",
            &format!("/api/auth/sessions/{other}"),
            Some(&first),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/sessions/revoke-others",
            Some(&first),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, ..) = call(&app, "GET", "/api/auth/sessions", Some(&second), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (_, _, list) = call(&app, "GET", "/api/auth/sessions", Some(&first), None).await;
        assert_eq!(list.as_array().unwrap().len(), 1);

        // Ending the current session signs out.
        let id = list[0]["id"].as_str().unwrap();
        let (status, ..) = call(
            &app,
            "DELETE",
            &format!("/api/auth/sessions/{id}"),
            Some(&first),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, ..) = call(&app, "GET", "/api/auth/sessions", Some(&first), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn revoking_a_session_ends_its_event_stream() {
        let app = app();
        call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("admin", &pw("admin")),
        )
        .await;
        let streaming = login_cookie(&app, "admin", &pw("admin"), true).await;
        let admin = login_cookie(&app, "admin", &pw("admin"), true).await;

        let response = raw(
            &app,
            Request::builder()
                .uri("/api/events/sse")
                .header(header::COOKIE, &streaming)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let reader = tokio::spawn(async move {
            tokio::time::timeout(
                Duration::from_secs(10),
                axum::body::to_bytes(response.into_body(), usize::MAX),
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!reader.is_finished(), "the stream should stay open");

        // The setup session and the streaming one both belong to this user.
        call(
            &app,
            "POST",
            "/api/auth/sessions/revoke-others",
            Some(&admin),
            None,
        )
        .await;
        let outcome = reader.await.unwrap();
        assert!(outcome.is_ok(), "stream was not closed after revocation");
    }

    #[tokio::test]
    async fn sessions_slide_and_never_pass_the_absolute_cap() {
        let (_, state) = app_state(None);
        let db = &state.db;
        users::create_first_admin(db, "a@example.com", None, &random_token()).unwrap();
        let origin = users::SessionOrigin {
            remember: true,
            ..Default::default()
        };
        users::create_session(db, "a@example.com", "tok", &origin).unwrap();
        let (_, mut session) = users::user_for_session(db, "tok").unwrap().unwrap();

        // Used a moment ago: no write.
        assert!(!users::touch_session(db, &session).unwrap());

        // Idle for a while: the expiry slides forward.
        session.last_used_at -= chrono::Duration::minutes(5);
        assert!(users::touch_session(db, &session).unwrap());
        let (_, slid) = users::user_for_session(db, "tok").unwrap().unwrap();
        assert!(slid.expires_at > session.expires_at);

        // An ancient session is capped at created_at + MAX_SESSION_DAYS.
        session.created_at -=
            chrono::Duration::days(users::MAX_SESSION_DAYS) - chrono::Duration::hours(1);
        session.last_used_at -= chrono::Duration::minutes(5);
        users::touch_session(db, &session).unwrap();
        let (_, capped) = users::user_for_session(db, "tok").unwrap().unwrap();
        assert!(
            capped.expires_at
                <= session.created_at + chrono::Duration::days(users::MAX_SESSION_DAYS)
        );
    }

    #[tokio::test]
    async fn a_password_chosen_by_someone_else_must_be_replaced() {
        let app = app();
        let (_, admin, _) = call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("admin", &pw("admin")),
        )
        .await;
        let admin = admin.unwrap();
        let new_user = Some(
            json!({ "email": "kim@example.com", "password": pw("temporary"), "role": "viewer" }),
        );
        let (status, ..) = call(&app, "POST", "/api/users", Some(&admin), new_user).await;
        assert_eq!(status, StatusCode::OK);

        let kim = login_cookie(&app, "kim@example.com", &pw("temporary"), true).await;
        let (_, _, status) = call(&app, "GET", "/api/auth/status", Some(&kim), None).await;
        assert_eq!(status["user"]["must_change_password"], true);

        // Everything but changing the password (or leaving) is refused.
        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&kim), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, ..) = call(&app, "GET", "/api/auth/sessions", Some(&kim), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let change =
            |new: &str| Some(json!({ "current_password": pw("temporary"), "new_password": new }));
        let (status, ..) = call(
            &app,
            "POST",
            "/api/auth/password",
            Some(&kim),
            change(&pw("temporary")),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, fresh, _) = call(
            &app,
            "POST",
            "/api/auth/password",
            Some(&kim),
            change(&pw("chosen")),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let kim = fresh.unwrap();
        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&kim), None).await;
        assert_eq!(status, StatusCode::OK);
        let (_, _, status) = call(&app, "GET", "/api/auth/status", Some(&kim), None).await;
        assert_eq!(status["user"]["must_change_password"], false);

        // An admin reset marks it again, unless they opt out or reset their own.
        let reset = |require: Option<bool>| {
            Some(json!({ "password": pw("reset"), "require_change": require }))
        };
        let (status, ..) = call(
            &app,
            "PUT",
            "/api/users/kim@example.com",
            Some(&admin),
            reset(None),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&kim), None).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "the reset ends their sessions"
        );
        let kim = login_cookie(&app, "kim@example.com", &pw("reset"), true).await;
        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&kim), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, ..) = call(
            &app,
            "PUT",
            "/api/users/kim@example.com",
            Some(&admin),
            reset(Some(false)),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let kim = login_cookie(&app, "kim@example.com", &pw("reset"), true).await;
        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&kim), None).await;
        assert_eq!(status, StatusCode::OK);

        let (status, ..) = call(
            &app,
            "PUT",
            "/api/users/admin@example.com",
            Some(&admin),
            reset(None),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let admin = login_cookie(&app, "admin", &pw("reset"), true).await;
        let (status, ..) = call(&app, "GET", "/api/webhooks", Some(&admin), None).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "resetting your own password is not forced"
        );
    }

    #[tokio::test]
    async fn security_events_reach_the_admin_audit_log_only() {
        let app = app();
        call(
            &app,
            "POST",
            "/api/auth/setup",
            None,
            creds("admin", &pw("admin")),
        )
        .await;
        call(
            &app,
            "POST",
            "/api/auth/login",
            None,
            login_creds("admin", &pw("wrong")),
        )
        .await;
        let admin = login_cookie(&app, "admin", &pw("admin"), true).await;
        let new_user = Some(json!({
            "email": "v@example.com", "password": pw("user"), "role": "viewer", "require_change": false,
        }));
        call(&app, "POST", "/api/users", Some(&admin), new_user).await;
        let viewer = login_cookie(&app, "v@example.com", &pw("user"), true).await;
        call(
            &app,
            "PUT",
            "/api/users/v@example.com",
            Some(&admin),
            Some(json!({ "role": "admin" })),
        )
        .await;
        call(
            &app,
            "DELETE",
            "/api/users/v@example.com",
            Some(&admin),
            None,
        )
        .await;

        let (status, ..) = call(&app, "GET", "/api/auth/audit", Some(&viewer), None).await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "the deleted viewer is signed out"
        );

        let (status, _, log) =
            call(&app, "GET", "/api/auth/audit?limit=50", Some(&admin), None).await;
        assert_eq!(status, StatusCode::OK);
        let kinds: Vec<&str> = log
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["kind"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "user_deleted",
                "role_changed",
                "login_succeeded",
                "user_created",
                "login_succeeded",
                "login_failed",
                "setup_completed",
            ]
        );
        let newest = &log[0];
        assert_eq!(newest["actor"], "admin@example.com");
        assert_eq!(newest["target"], "v@example.com");
        let failed = &log[5];
        assert_eq!(failed["actor"], "admin", "a failure records what was typed");
        assert!(
            log.as_array()
                .unwrap()
                .iter()
                .all(|e| e.get("password").is_none())
        );

        let (_, _, limited) =
            call(&app, "GET", "/api/auth/audit?limit=2", Some(&admin), None).await;
        assert_eq!(limited.as_array().unwrap().len(), 2);

        // A viewer cannot read it.
        let other = Some(json!({
            "email": "w@example.com", "password": pw("user"), "role": "viewer", "require_change": false,
        }));
        call(&app, "POST", "/api/users", Some(&admin), other).await;
        let viewer = login_cookie(&app, "w@example.com", &pw("user"), true).await;
        let (status, ..) = call(&app, "GET", "/api/auth/audit", Some(&viewer), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
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
            creds("admin", &pw("admin")),
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
                    json!({ "login": "a", "password": pw("wrong") }).to_string(),
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
                    json!({ "token": setup_token(), "email": "admin@example.com", "username": "admin", "password": pw("admin") }).to_string(),
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
    fn forwarded_for_is_believed_only_from_trusted_proxies() {
        let ip = |s: &str| Some(s.parse::<IpAddr>().unwrap());
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "6.6.6.6, 198.51.100.1, 10.0.0.7".parse().unwrap(),
        );
        // Default: only loopback is trusted. A proxy in the 10/8 network is a
        // plain client, so its header is ignored.
        let default = TrustedProxies::default();
        assert_eq!(
            resolve_client_ip(&default, &headers, ip("10.0.0.9")),
            ip("10.0.0.9")
        );
        assert_eq!(
            resolve_client_ip(&default, &headers, ip("127.0.0.1")),
            ip("10.0.0.7")
        );
        // With 10/8 trusted, hops are skipped from the right until a stranger.
        let trusted = TrustedProxies::parse(["10.0.0.0/8"]).unwrap();
        assert_eq!(
            resolve_client_ip(&trusted, &headers, ip("10.0.0.9")),
            ip("198.51.100.1")
        );
        // A spoofed leftmost entry is never reached.
        assert_ne!(
            resolve_client_ip(&trusted, &headers, ip("10.0.0.9")),
            ip("6.6.6.6")
        );
        // An unparseable hop or a missing header falls back to the peer.
        headers.insert("x-forwarded-for", "garbage".parse().unwrap());
        assert_eq!(
            resolve_client_ip(&trusted, &headers, ip("10.0.0.9")),
            ip("10.0.0.9")
        );
        assert_eq!(
            resolve_client_ip(&trusted, &HeaderMap::new(), ip("10.0.0.9")),
            ip("10.0.0.9")
        );
        assert_eq!(resolve_client_ip(&trusted, &headers, None), None);
    }
}
