//! Records security-relevant account events for the admin-only audit log.

use std::net::IpAddr;

use crate::db::auth_events;
use crate::web::state::AppState;

#[derive(Debug, Clone, Copy)]
pub enum AuditKind {
    SetupCompleted,
    RecoveryUsed,
    BootstrapTokenFailed,
    LoginSucceeded,
    LoginFailed,
    LoginThrottled,
    PasswordChanged,
    PasswordReset,
    UserCreated,
    UserDeleted,
    RoleChanged,
    SessionRevoked,
    OtherSessionsRevoked,
    ProfileUpdated,
    AccountDisabled,
    AccountEnabled,
}

impl AuditKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SetupCompleted => "setup_completed",
            Self::RecoveryUsed => "recovery_used",
            Self::BootstrapTokenFailed => "bootstrap_token_failed",
            Self::LoginSucceeded => "login_succeeded",
            Self::LoginFailed => "login_failed",
            Self::LoginThrottled => "login_throttled",
            Self::PasswordChanged => "password_changed",
            Self::PasswordReset => "password_reset",
            Self::UserCreated => "user_created",
            Self::UserDeleted => "user_deleted",
            Self::RoleChanged => "role_changed",
            Self::SessionRevoked => "session_revoked",
            Self::OtherSessionsRevoked => "other_sessions_revoked",
            Self::ProfileUpdated => "profile_updated",
            Self::AccountDisabled => "account_disabled",
            Self::AccountEnabled => "account_enabled",
        }
    }
}

/// Free text comes from clients (a mistyped login, say), so it is capped
/// and stripped of control characters.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// Appends one event. A failure to write is logged and never fails the
/// request: the audit trail must not become a way to block sign-ins.
pub async fn record(
    state: &AppState,
    kind: AuditKind,
    actor: Option<&str>,
    target: Option<&str>,
    ip: Option<IpAddr>,
    detail: Option<&str>,
) {
    let db = state.db.clone();
    let (actor, target, detail) = (actor.map(clean), target.map(clean), detail.map(clean));
    let ip = ip.map(|ip| ip.to_string());
    let result = tokio::task::spawn_blocking(move || {
        auth_events::insert(
            &db,
            kind.as_str(),
            actor.as_deref(),
            target.as_deref(),
            ip.as_deref(),
            detail.as_deref(),
        )
    })
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "failed to record audit event"),
        Err(error) => tracing::warn!(%error, "audit task failed"),
    }
}
