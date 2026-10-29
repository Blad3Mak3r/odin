//! Dashboard accounts and their cookie sessions. Only SHA-256 digests of
//! session tokens are stored; password hashing lives in `web::auth`.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use uuid::Uuid;

use super::Db;

/// Idle timeout of a "remember me" session.
pub const REMEMBER_IDLE_DAYS: i64 = 14;
/// Idle timeout of a session that was not remembered; its cookie also dies
/// with the browser.
pub const SHORT_IDLE_HOURS: i64 = 12;
/// No session outlives this, however actively it is used.
pub const MAX_SESSION_DAYS: i64 = 90;
/// `last_used_at` and the expiry are refreshed at most this often.
const TOUCH_INTERVAL_SECS: i64 = 60;

#[derive(Debug, Error)]
pub enum UserError {
    #[error("user '{0}' not found")]
    NotFound(String),
    #[error("an account with the email '{0}' already exists")]
    EmailTaken(String),
    #[error("the username '{0}' is already taken")]
    UsernameTaken(String),
    #[error("at least one administrator must remain")]
    LastAdmin,
    #[error("initial setup has already been completed")]
    SetupDone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Viewer,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Viewer => "viewer",
        }
    }

    fn parse(value: &str) -> Self {
        if value == "admin" {
            Role::Admin
        } else {
            Role::Viewer
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct User {
    /// Primary key, stored lowercase.
    pub email: String,
    /// Optional alternative login handle, unique when set.
    pub username: Option<String>,
    pub role: Role,
    pub created_at: DateTime<Utc>,
    /// The password was set by someone else and must be replaced at sign-in.
    pub must_change_password: bool,
    /// Cannot sign in; its sessions are gone.
    pub disabled: bool,
    pub last_login_at: Option<DateTime<Utc>>,
}

/// A user together with the stored password hash, for login only.
#[derive(Debug, Clone)]
pub struct UserCredentials {
    pub user: User,
    pub password_hash: String,
}

pub fn count(db: &Db) -> Result<u64> {
    let n: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
    Ok(n as u64)
}

/// Creates the first administrator. Atomic, so concurrent setup requests
/// cannot both succeed.
pub fn create_first_admin(
    db: &Db,
    email: &str,
    username: Option<&str>,
    password_hash: &str,
) -> Result<User> {
    let now = Utc::now();
    let inserted = db.conn().execute(
        "INSERT INTO users (email, username, password_hash, role, created_at) \
         SELECT ?1, ?2, ?3, 'admin', ?4 WHERE NOT EXISTS (SELECT 1 FROM users)",
        params![email, username, password_hash, now],
    )?;
    if inserted == 0 {
        return Err(UserError::SetupDone.into());
    }
    Ok(User {
        email: email.to_string(),
        username: username.map(str::to_string),
        role: Role::Admin,
        created_at: now,
        must_change_password: false,
        disabled: false,
        last_login_at: None,
    })
}

pub fn create(
    db: &Db,
    email: &str,
    username: Option<&str>,
    password_hash: &str,
    role: Role,
    must_change_password: bool,
) -> Result<User> {
    let now = Utc::now();
    let result = db.conn().execute(
        "INSERT INTO users (email, username, password_hash, role, created_at, must_change_password) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            email,
            username,
            password_hash,
            role.as_str(),
            now,
            must_change_password
        ],
    );
    match result {
        Ok(_) => Ok(User {
            email: email.to_string(),
            username: username.map(str::to_string),
            role,
            created_at: now,
            must_change_password,
            disabled: false,
            last_login_at: None,
        }),
        Err(rusqlite::Error::SqliteFailure(err, Some(message)))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            if message.contains("users.username") {
                Err(UserError::UsernameTaken(username.unwrap_or_default().to_string()).into())
            } else {
                Err(UserError::EmailTaken(email.to_string()).into())
            }
        }
        Err(err) => Err(err.into()),
    }
}

/// Looks an account up by email or username, whichever `login` matches.
pub fn find_credentials(db: &Db, login: &str) -> Result<Option<UserCredentials>> {
    db.conn()
        .query_row(
            "SELECT email, username, role, created_at, must_change_password, disabled, last_login_at, password_hash FROM users \
             WHERE email = ?1 OR username = ?1",
            params![login],
            |row| {
                Ok(UserCredentials {
                    user: row_to_user(row)?,
                    password_hash: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn password_hash(db: &Db, email: &str) -> Result<Option<String>> {
    db.conn()
        .query_row(
            "SELECT password_hash FROM users WHERE email = ?1",
            params![email],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

pub fn list(db: &Db) -> Result<Vec<User>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT email, username, role, created_at, must_change_password, disabled, last_login_at FROM users ORDER BY COALESCE(username, email) COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], row_to_user)?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

/// Changes a user's role; refuses to demote the last enabled administrator.
pub fn set_role(db: &Db, email: &str, role: Role) -> Result<()> {
    let conn = db.conn();
    let current = role_and_state(&conn, email)?;
    let Some((current, disabled)) = current else {
        return Err(UserError::NotFound(email.to_string()).into());
    };
    if Role::parse(&current) == Role::Admin
        && !disabled
        && role != Role::Admin
        && admin_count(&conn)? <= 1
    {
        return Err(UserError::LastAdmin.into());
    }
    conn.execute(
        "UPDATE users SET role = ?2 WHERE email = ?1",
        params![email, role.as_str()],
    )?;
    Ok(())
}

/// Replaces the password and revokes every session of that user, returning
/// the revoked token hashes. `must_change` marks a password someone else chose.
pub fn set_password(
    db: &Db,
    email: &str,
    password_hash: &str,
    must_change: bool,
) -> Result<Vec<String>> {
    let conn = db.conn();
    let updated = conn.execute(
        "UPDATE users SET password_hash = ?2, must_change_password = ?3 WHERE email = ?1",
        params![email, password_hash, must_change],
    )?;
    if updated == 0 {
        return Err(UserError::NotFound(email.to_string()).into());
    }
    let hashes = session_hashes(&conn, email, None)?;
    conn.execute(
        "DELETE FROM auth_sessions WHERE user_email = ?1",
        params![email],
    )?;
    Ok(hashes)
}

/// Deletes a user (their sessions cascade); refuses the last administrator.
/// Returns the token hashes of the sessions that went with it.
pub fn delete(db: &Db, email: &str) -> Result<Vec<String>> {
    let conn = db.conn();
    let Some((role, disabled)) = role_and_state(&conn, email)? else {
        return Err(UserError::NotFound(email.to_string()).into());
    };
    if Role::parse(&role) == Role::Admin && !disabled && admin_count(&conn)? <= 1 {
        return Err(UserError::LastAdmin.into());
    }
    let hashes = session_hashes(&conn, email, None)?;
    conn.execute("DELETE FROM users WHERE email = ?1", params![email])?;
    Ok(hashes)
}

/// Enables or disables an account. Disabling signs it out everywhere and
/// returns the revoked token hashes; the last enabled administrator cannot
/// be disabled.
pub fn set_disabled(db: &Db, email: &str, disabled: bool) -> Result<Vec<String>> {
    let conn = db.conn();
    let Some((role, was_disabled)) = role_and_state(&conn, email)? else {
        return Err(UserError::NotFound(email.to_string()).into());
    };
    if disabled && Role::parse(&role) == Role::Admin && !was_disabled && admin_count(&conn)? <= 1 {
        return Err(UserError::LastAdmin.into());
    }
    conn.execute(
        "UPDATE users SET disabled = ?2 WHERE email = ?1",
        params![email, disabled],
    )?;
    if !disabled {
        return Ok(Vec::new());
    }
    let hashes = session_hashes(&conn, email, None)?;
    conn.execute(
        "DELETE FROM auth_sessions WHERE user_email = ?1",
        params![email],
    )?;
    Ok(hashes)
}

/// Sets or clears (`None`) the optional username.
pub fn set_username(db: &Db, email: &str, username: Option<&str>) -> Result<()> {
    let result = db.conn().execute(
        "UPDATE users SET username = ?2 WHERE email = ?1",
        params![email, username],
    );
    match result {
        Ok(0) => Err(UserError::NotFound(email.to_string()).into()),
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(UserError::UsernameTaken(username.unwrap_or_default().to_string()).into())
        }
        Err(err) => Err(err.into()),
    }
}

/// Changes the primary key. Sessions follow through `ON UPDATE CASCADE`.
pub fn set_email(db: &Db, email: &str, new_email: &str) -> Result<()> {
    let result = db.conn().execute(
        "UPDATE users SET email = ?2 WHERE email = ?1",
        params![email, new_email],
    );
    match result {
        Ok(0) => Err(UserError::NotFound(email.to_string()).into()),
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(UserError::EmailTaken(new_email.to_string()).into())
        }
        Err(err) => Err(err.into()),
    }
}

pub fn record_login(db: &Db, email: &str) -> Result<()> {
    db.conn().execute(
        "UPDATE users SET last_login_at = ?2 WHERE email = ?1",
        params![email, Utc::now()],
    )?;
    Ok(())
}

pub fn find_by_email(db: &Db, email: &str) -> Result<Option<User>> {
    db.conn()
        .query_row(
            "SELECT email, username, role, created_at, must_change_password, disabled, last_login_at \
             FROM users WHERE email = ?1",
            params![email],
            row_to_user,
        )
        .optional()
        .map_err(Into::into)
}

/// One login session, as stored. `token_hash` never leaves the server.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(skip)]
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub remember: bool,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// What the server knows about the client when a session starts.
#[derive(Debug, Clone, Default)]
pub struct SessionOrigin {
    pub remember: bool,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

impl SessionInfo {
    pub fn idle_timeout(&self) -> Duration {
        idle_timeout(self.remember)
    }
}

fn idle_timeout(remember: bool) -> Duration {
    if remember {
        Duration::days(REMEMBER_IDLE_DAYS)
    } else {
        Duration::hours(SHORT_IDLE_HOURS)
    }
}

pub fn create_session(
    db: &Db,
    user_email: &str,
    token_hash: &str,
    origin: &SessionOrigin,
) -> Result<()> {
    let now = Utc::now();
    let conn = db.conn();
    conn.execute(
        "DELETE FROM auth_sessions WHERE expires_at <= ?1",
        params![now],
    )?;
    conn.execute(
        "INSERT INTO auth_sessions \
         (token_hash, id, user_email, created_at, last_used_at, expires_at, remember, ip, user_agent) \
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7, ?8)",
        params![
            token_hash,
            Uuid::new_v4().to_string(),
            user_email,
            now,
            now + idle_timeout(origin.remember),
            origin.remember,
            origin.ip,
            origin.user_agent,
        ],
    )?;
    Ok(())
}

const SESSION_COLUMNS: &str = "s.id, s.token_hash, s.created_at, s.last_used_at, s.expires_at, \
                               s.remember, s.ip, s.user_agent";

fn row_to_session(row: &rusqlite::Row, offset: usize) -> rusqlite::Result<SessionInfo> {
    Ok(SessionInfo {
        id: row.get(offset)?,
        token_hash: row.get(offset + 1)?,
        created_at: row.get(offset + 2)?,
        last_used_at: row.get(offset + 3)?,
        expires_at: row.get(offset + 4)?,
        remember: row.get(offset + 5)?,
        ip: row.get(offset + 6)?,
        user_agent: row.get(offset + 7)?,
    })
}

pub fn user_for_session(db: &Db, token_hash: &str) -> Result<Option<(User, SessionInfo)>> {
    db.conn()
        .query_row(
            &format!(
                "SELECT u.email, u.username, u.role, u.created_at, u.must_change_password, u.disabled, u.last_login_at, {SESSION_COLUMNS} \
                 FROM auth_sessions s JOIN users u ON u.email = s.user_email \
                 WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND u.disabled = 0"
            ),
            params![token_hash, Utc::now()],
            |row| Ok((row_to_user(row)?, row_to_session(row, 7)?)),
        )
        .optional()
        .map_err(Into::into)
}

/// Slides the session's expiry forward, at most once per minute and never
/// past the absolute cap. Returns whether it was refreshed.
pub fn touch_session(db: &Db, session: &SessionInfo) -> Result<bool> {
    let now = Utc::now();
    if now - session.last_used_at < Duration::seconds(TOUCH_INTERVAL_SECS) {
        return Ok(false);
    }
    let expires =
        (now + session.idle_timeout()).min(session.created_at + Duration::days(MAX_SESSION_DAYS));
    db.conn().execute(
        "UPDATE auth_sessions SET last_used_at = ?2, expires_at = ?3 WHERE token_hash = ?1",
        params![session.token_hash, now, expires],
    )?;
    Ok(true)
}

pub fn list_sessions(db: &Db, user_email: &str) -> Result<Vec<SessionInfo>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(&format!(
        "SELECT {SESSION_COLUMNS} FROM auth_sessions s \
         WHERE s.user_email = ?1 AND s.expires_at > ?2 ORDER BY s.last_used_at DESC"
    ))?;
    let rows = stmt.query_map(params![user_email, Utc::now()], |row| {
        row_to_session(row, 0)
    })?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

/// Deletes one of the user's sessions by its public id; returns its token
/// hash so live streams can be closed, or `None` if it is not theirs.
pub fn delete_session_by_id(db: &Db, user_email: &str, id: &str) -> Result<Option<String>> {
    let conn = db.conn();
    let hash: Option<String> = conn
        .query_row(
            "SELECT token_hash FROM auth_sessions WHERE id = ?1 AND user_email = ?2",
            params![id, user_email],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(hash) = &hash {
        conn.execute(
            "DELETE FROM auth_sessions WHERE token_hash = ?1",
            params![hash],
        )?;
    }
    Ok(hash)
}

/// Deletes every session of the user except `keep`; returns the revoked hashes.
pub fn delete_other_sessions(db: &Db, user_email: &str, keep: &str) -> Result<Vec<String>> {
    let conn = db.conn();
    let hashes = session_hashes(&conn, user_email, Some(keep))?;
    conn.execute(
        "DELETE FROM auth_sessions WHERE user_email = ?1 AND token_hash != ?2",
        params![user_email, keep],
    )?;
    Ok(hashes)
}

pub fn delete_session(db: &Db, token_hash: &str) -> Result<()> {
    db.conn().execute(
        "DELETE FROM auth_sessions WHERE token_hash = ?1",
        params![token_hash],
    )?;
    Ok(())
}

fn session_hashes(
    conn: &rusqlite::Connection,
    user_email: &str,
    except: Option<&str>,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT token_hash FROM auth_sessions WHERE user_email = ?1 AND token_hash != ?2",
    )?;
    stmt.query_map(params![user_email, except.unwrap_or("")], |row| row.get(0))?
        .collect()
}

fn role_and_state(
    conn: &rusqlite::Connection,
    email: &str,
) -> rusqlite::Result<Option<(String, bool)>> {
    conn.query_row(
        "SELECT role, disabled FROM users WHERE email = ?1",
        params![email],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

fn admin_count(conn: &rusqlite::Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM users WHERE role = 'admin' AND disabled = 0",
        [],
        |r| r.get(0),
    )
}

fn row_to_user(row: &rusqlite::Row) -> rusqlite::Result<User> {
    let role: String = row.get(2)?;
    Ok(User {
        email: row.get(0)?,
        username: row.get(1)?,
        role: Role::parse(&role),
        created_at: row.get(3)?,
        must_change_password: row.get(4)?,
        disabled: row.get(5)?,
        last_login_at: row.get(6)?,
    })
}
