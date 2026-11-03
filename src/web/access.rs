//! What viewers and operators may reach. Administrators are not restricted
//! and never get here. Everyone else sees only the instances an
//! administrator granted them, and operators may additionally run those
//! instances, but never change anything global or hold a secret.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use axum::response::Response;

use crate::db::instance_access::Granted;
use crate::db::users::Role;
use crate::game::GameId;

/// Which instances the caller may see, inserted into request extensions by
/// the auth middleware for handlers that list or stream across instances.
#[derive(Debug, Clone)]
pub enum InstanceAccess {
    All,
    Only(Arc<Granted>),
}

impl InstanceAccess {
    pub fn permits_id(&self, id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(granted) => granted.ids.contains(id),
        }
    }

    pub fn permits_name(&self, game: GameId, name: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(granted) => granted.names.contains(&(game, name.to_string())),
        }
    }

    /// An entry that carries either identity (activity and telemetry have
    /// the UUID only sometimes).
    pub fn permits_entry(&self, id: Option<&str>, game: GameId, name: &str) -> bool {
        match (self, id) {
            (Self::All, _) => true,
            (_, Some(id)) => self.permits_id(id),
            (_, None) => self.permits_name(game, name),
        }
    }
}

pub const ADMIN_ONLY: &str = "this area is for administrators";
pub const NOT_ALLOWED: &str = "this action requires an administrator";
pub const NO_SUCH_INSTANCE: &str = "game instance does not exist";

/// What an `/api` path points at.
enum Target<'a> {
    /// `/api/instances/{id}[/rest]`.
    Instance {
        id: &'a str,
        rest: &'a str,
    },
    /// Areas that list or edit across instances and are not filtered.
    AdminArea,
    Other,
}

fn target(path: &str) -> Target<'_> {
    if path == "/api/instances"
        || path == "/api/ark-clusters"
        || path.starts_with("/api/ark-clusters/")
    {
        return Target::AdminArea;
    }
    match path.strip_prefix("/api/instances/") {
        Some(tail) => {
            let (id, rest) = tail.split_once('/').unwrap_or((tail, ""));
            if id == "bulk" {
                Target::AdminArea
            } else {
                Target::Instance { id, rest }
            }
        }
        None => Target::Other,
    }
}

/// Whether `role` (never an administrator) may make this request, given the
/// instances granted to it. The error is the status and message to answer
/// with; an instance outside the grant looks exactly like one that does not
/// exist.
pub fn authorize(
    role: Role,
    access: &InstanceAccess,
    method: &Method,
    path: &str,
    own_account_path: bool,
) -> Result<(), (StatusCode, &'static str)> {
    let safe = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    match target(path) {
        Target::AdminArea => Err((StatusCode::FORBIDDEN, ADMIN_ONLY)),
        Target::Instance { id, rest } => {
            if !access.permits_id(id) {
                return Err((StatusCode::NOT_FOUND, NO_SUCH_INSTANCE));
            }
            if safe || (role == Role::Operator && operator_may_mutate(method, rest)) {
                Ok(())
            } else {
                Err((StatusCode::FORBIDDEN, NOT_ALLOWED))
            }
        }
        Target::Other => {
            if safe || own_account_path {
                Ok(())
            } else {
                Err((StatusCode::FORBIDDEN, NOT_ALLOWED))
            }
        }
    }
}

/// The changes an operator may make inside an instance it was granted. This
/// is an allowlist: a route added later stays administrator-only until it is
/// listed here. Left out on purpose are what is global or sensitive, such as
/// deleting, renaming or cloning an instance, its resource limits, its backup
/// storage credentials, moving it to the shared install, and a full wipe.
/// Raw configuration files (which hold passwords in the clear) are refused
/// for every non-administrator before this is consulted.
fn operator_may_mutate(method: &Method, rest: &str) -> bool {
    let edits = matches!(*method, Method::POST | Method::PUT | Method::PATCH);
    let segments: Vec<&str> = rest.split('/').collect();
    match segments.as_slice() {
        ["start" | "stop" | "restart" | "tags" | "uptime-schedule" | "backup-schedule"]
        | ["config"]
        | ["config", "advanced"]
        | ["config", "advanced", "init"]
        | ["backups"]
        | ["backups", "jobs"]
        | ["runtime", "repair" | "update"] => edits,
        ["backups", _, "restore"] | ["backups", _, "restore", "job"] => *method == Method::POST,
        ["backups", _] => *method == Method::DELETE,
        ["saves", ..] => true,
        ["valheim" | "rust", "clone"] | ["rust", "full-wipe"] => false,
        ["valheim" | "rust" | "7d2d" | "palworld", ..] => true,
        _ => false,
    }
}

/// Requests whose body can carry a secret an operator must not set.
pub fn is_config_write(method: &Method, path: &str) -> bool {
    let Target::Instance { rest, .. } = target(path) else {
        return false;
    };
    matches!(*method, Method::POST | Method::PUT | Method::PATCH)
        && matches!(rest, "config" | "config/advanced")
}

/// Largest configuration body an operator's request is rewritten in memory.
const MAX_BODY_BYTES: usize = 1024 * 1024;

fn is_secret_field(name: &str) -> bool {
    crate::game::config_documents::is_sensitive_key(name)
        || name.to_ascii_lowercase().contains("gslt")
}

/// Drops what would set a secret: fields named like one, and the entries of
/// a list of `{ key, value }` changes whose key is one. The rest of the
/// request goes through, so an operator can still save the other settings.
pub fn strip_secrets(value: &mut serde_json::Value) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            // `key` is the name half of a `{ key, value }` change, not a secret.
            map.retain(|name, _| name == "key" || !is_secret_field(name));
            map.values_mut().for_each(strip_secrets);
        }
        Value::Array(items) => {
            items.retain(|item| {
                item.get("key")
                    .and_then(Value::as_str)
                    .is_none_or(|key| !is_secret_field(key))
            });
            items.iter_mut().for_each(strip_secrets);
        }
        _ => {}
    }
}

/// Rewrites an operator's configuration request without its secrets.
pub async fn strip_request_secrets(req: Request) -> Result<Request, Response> {
    let (mut parts, body) = req.into_parts();
    let refused = |status: StatusCode, message: &str| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "error": message }).to_string(),
            ))
            .unwrap_or_default()
    };
    let Ok(bytes) = axum::body::to_bytes(body, MAX_BODY_BYTES).await else {
        return Err(refused(StatusCode::PAYLOAD_TOO_LARGE, "request too large"));
    };
    let bytes = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(mut json) => {
            strip_secrets(&mut json);
            serde_json::to_vec(&json).map(Into::into).unwrap_or(bytes)
        }
        // Not JSON: the handler will refuse it as it would anyone's.
        Err(_) => bytes,
    };
    parts.headers.remove(header::CONTENT_LENGTH);
    Ok(Request::from_parts(parts, Body::from(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn only(ids: &[&str]) -> InstanceAccess {
        InstanceAccess::Only(Arc::new(Granted {
            ids: ids.iter().map(|id| id.to_string()).collect(),
            names: HashSet::new(),
        }))
    }

    fn check(role: Role, method: Method, path: &str) -> Result<(), StatusCode> {
        authorize(role, &only(&["mine"]), &method, path, false).map_err(|(status, _)| status)
    }

    #[test]
    fn instances_outside_the_grant_do_not_exist() {
        for role in [Role::Viewer, Role::Operator] {
            assert_eq!(
                check(role, Method::GET, "/api/instances/other"),
                Err(StatusCode::NOT_FOUND)
            );
            assert_eq!(
                check(role, Method::POST, "/api/instances/other/start"),
                Err(StatusCode::NOT_FOUND)
            );
            assert_eq!(check(role, Method::GET, "/api/instances/mine/logs"), Ok(()));
        }
    }

    #[test]
    fn viewers_never_change_an_instance() {
        assert_eq!(
            check(Role::Viewer, Method::POST, "/api/instances/mine/start"),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn operators_run_their_instances_but_not_what_is_sensitive() {
        let allowed = [
            (Method::POST, "start"),
            (Method::POST, "restart"),
            (Method::PUT, "config"),
            (Method::PUT, "config/advanced"),
            (Method::POST, "backups/jobs"),
            (Method::POST, "backups/b1/restore/job"),
            (Method::DELETE, "backups/b1"),
            (Method::PUT, "valheim/mods/x/disable"),
            (Method::POST, "7d2d/players/bob/kick"),
            (Method::DELETE, "saves/world.db"),
        ];
        for (method, rest) in allowed {
            assert_eq!(
                check(
                    Role::Operator,
                    method.clone(),
                    &format!("/api/instances/mine/{rest}")
                ),
                Ok(()),
                "{method} {rest}"
            );
        }
        let refused = [
            (Method::DELETE, ""),
            (Method::POST, "rename"),
            (Method::PUT, "resource-limits"),
            (Method::PUT, "backup-storage"),
            (Method::POST, "shared-install"),
            (Method::POST, "rust/clone"),
            (Method::POST, "valheim/clone"),
            (Method::POST, "rust/full-wipe"),
            (Method::DELETE, "config"),
            (Method::POST, "something-new"),
        ];
        for (method, rest) in refused {
            let path = format!("/api/instances/mine/{rest}");
            assert_eq!(
                check(Role::Operator, method.clone(), path.trim_end_matches('/')),
                Err(StatusCode::FORBIDDEN),
                "{method} {rest}"
            );
        }
    }

    #[test]
    fn cross_instance_areas_are_administrator_only() {
        for path in [
            "/api/instances",
            "/api/instances/bulk/mods/update",
            "/api/ark-clusters",
            "/api/ark-clusters/c1/files",
        ] {
            for role in [Role::Viewer, Role::Operator] {
                assert_eq!(
                    check(role, Method::GET, path),
                    Err(StatusCode::FORBIDDEN),
                    "{path}"
                );
            }
        }
    }

    #[test]
    fn elsewhere_only_reads_and_the_own_account_are_open() {
        for role in [Role::Viewer, Role::Operator] {
            assert_eq!(check(role, Method::GET, "/api/webhooks"), Ok(()));
            assert_eq!(
                check(role, Method::POST, "/api/webhooks"),
                Err(StatusCode::FORBIDDEN)
            );
            assert_eq!(
                check(role, Method::POST, "/api/games/rust/instances"),
                Err(StatusCode::FORBIDDEN)
            );
            assert_eq!(
                authorize(role, &only(&[]), &Method::POST, "/api/auth/password", true),
                Ok(())
            );
        }
    }

    #[test]
    fn operator_config_requests_lose_their_secrets() {
        let mut body = serde_json::json!({
            "port": 2456,
            "password": "hunter2",
            "rcon_password": "x",
            "changes": [
                { "file": "a", "key": "ServerAdminPassword", "value": "x" },
                { "file": "a", "key": "MaxPlayers", "value": "10" },
            ],
        });
        strip_secrets(&mut body);
        assert_eq!(
            body,
            serde_json::json!({
                "port": 2456,
                "changes": [{ "file": "a", "key": "MaxPlayers", "value": "10" }],
            })
        );
        assert!(is_config_write(&Method::PUT, "/api/instances/i/config"));
        assert!(is_config_write(
            &Method::PUT,
            "/api/instances/i/config/advanced"
        ));
        assert!(!is_config_write(&Method::GET, "/api/instances/i/config"));
        assert!(!is_config_write(&Method::PUT, "/api/instances/i/tags"));
    }
}
