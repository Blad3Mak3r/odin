//! Emailing a one-time link that lets an invited user choose their password.

use std::net::IpAddr;

use serde::Serialize;

use crate::db::password_links;
use crate::web::audit::{self, AuditKind};
use crate::web::auth::new_invitation_token;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

/// Where the dashboard's accept-invitation page lives. The token goes in the
/// fragment, which browsers never send to the server, so it stays out of
/// access logs and `Referer` headers.
const ACCEPT_PATH: &str = "/accept-invite";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    Sent,
    /// The link exists but the mail server did not take the message; the
    /// administrator can resend.
    Failed,
}

#[derive(Serialize)]
pub struct Resent {
    pub invitation_delivery: Delivery,
}

/// Refuses to start an invitation when it could not be delivered or trusted.
pub fn ensure_available(state: &AppState) -> Result<(), BadRequest> {
    if state.invitations_available() {
        return Ok(());
    }
    Err(BadRequest(
        "email invitations are not set up: configure [smtp] and [server].public_url, or set a password"
            .into(),
    ))
}

/// Replaces `email`'s link with a fresh one and mails it. A delivery failure
/// is reported, not raised: the account and its link already exist.
pub async fn issue_and_send(
    state: &AppState,
    actor: &str,
    email: &str,
    ip: Option<IpAddr>,
) -> ApiResult<Delivery> {
    ensure_available(state)?;
    let (mailer, origin) = match (&state.mail, &state.public_origin) {
        (Some(mailer), Some(origin)) => (mailer.clone(), origin.origin.clone()),
        _ => unreachable!("checked by ensure_available"),
    };
    let (token, token_hash) = new_invitation_token();
    let (db, target) = (state.db.clone(), email.to_string());
    run_blocking(move || {
        password_links::issue(&db, &target, &token_hash, password_links::VALID_FOR)
    })
    .await?;

    let link = format!("{origin}{ACCEPT_PATH}#token={token}");
    let to = email.to_string();
    let sent = tokio::task::spawn_blocking(move || {
        mailer.send_invitation(&to, &link, password_links::VALID_FOR)
    })
    .await;
    let (delivery, kind) = match sent {
        Ok(Ok(())) => (Delivery::Sent, AuditKind::InvitationSent),
        Ok(Err(error)) => {
            tracing::warn!(target = %email, error = %format!("{error:#}"), "could not send the invitation email");
            (Delivery::Failed, AuditKind::InvitationFailed)
        }
        Err(error) => {
            tracing::warn!(target = %email, %error, "invitation email task failed");
            (Delivery::Failed, AuditKind::InvitationFailed)
        }
    };
    audit::record(state, kind, Some(actor), Some(email), ip, None).await;
    Ok(delivery)
}
