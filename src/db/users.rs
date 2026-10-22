//! Dashboard accounts and their cookie sessions. Only SHA-256 digests of
//! session tokens are stored; password hashing lives in `web::auth`.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::Db;

pub const SESSION_LIFETIME_DAYS: i64 = 14;

#[derive(Debug, Error)]
pub enum UserError {
    #[error("user '{0}' not found")]
    NotFound(String),
    #[error("a user named '{0}' already exists")]
    Exists(String),
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
    pub id: String,
    pub username: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
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
pub fn create_first_admin(db: &Db, username: &str, password_hash: &str) -> Result<User> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let inserted = db.conn().execute(
        "INSERT INTO users (id, username, password_hash, role, created_at) \
         SELECT ?1, ?2, ?3, 'admin', ?4 WHERE NOT EXISTS (SELECT 1 FROM users)",
        params![id, username, password_hash, now],
    )?;
    if inserted == 0 {
        return Err(UserError::SetupDone.into());
    }
    Ok(User {
        id,
        username: username.to_string(),
        role: Role::Admin,
        created_at: now,
    })
}

pub fn create(db: &Db, username: &str, password_hash: &str, role: Role) -> Result<User> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let result = db.conn().execute(
        "INSERT INTO users (id, username, password_hash, role, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, username, password_hash, role.as_str(), now],
    );
    match result {
        Ok(_) => Ok(User {
            id,
            username: username.to_string(),
            role,
            created_at: now,
        }),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(UserError::Exists(username.to_string()).into())
        }
        Err(err) => Err(err.into()),
    }
}

pub fn find_credentials(db: &Db, username: &str) -> Result<Option<UserCredentials>> {
    db.conn()
        .query_row(
            "SELECT id, username, role, created_at, password_hash FROM users WHERE username = ?1",
            params![username],
            |row| {
                Ok(UserCredentials {
                    user: row_to_user(row)?,
                    password_hash: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn password_hash(db: &Db, id: &str) -> Result<Option<String>> {
    db.conn()
        .query_row(
            "SELECT password_hash FROM users WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

pub fn list(db: &Db) -> Result<Vec<User>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, username, role, created_at FROM users ORDER BY username COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], row_to_user)?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

/// Changes a user's role; refuses to demote the last administrator.
pub fn set_role(db: &Db, id: &str, role: Role) -> Result<()> {
    let conn = db.conn();
    let current: Option<String> = conn
        .query_row("SELECT role FROM users WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(current) = current else {
        return Err(UserError::NotFound(id.to_string()).into());
    };
    if Role::parse(&current) == Role::Admin && role != Role::Admin && admin_count(&conn)? <= 1 {
        return Err(UserError::LastAdmin.into());
    }
    conn.execute(
        "UPDATE users SET role = ?2 WHERE id = ?1",
        params![id, role.as_str()],
    )?;
    Ok(())
}

/// Replaces the password and revokes every session of that user.
pub fn set_password(db: &Db, id: &str, password_hash: &str) -> Result<()> {
    let conn = db.conn();
    let updated = conn.execute(
        "UPDATE users SET password_hash = ?2 WHERE id = ?1",
        params![id, password_hash],
    )?;
    if updated == 0 {
        return Err(UserError::NotFound(id.to_string()).into());
    }
    conn.execute("DELETE FROM auth_sessions WHERE user_id = ?1", params![id])?;
    Ok(())
}

/// Deletes a user (their sessions cascade); refuses the last administrator.
pub fn delete(db: &Db, id: &str) -> Result<()> {
    let conn = db.conn();
    let role: Option<String> = conn
        .query_row("SELECT role FROM users WHERE id = ?1", params![id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(role) = role else {
        return Err(UserError::NotFound(id.to_string()).into());
    };
    if Role::parse(&role) == Role::Admin && admin_count(&conn)? <= 1 {
        return Err(UserError::LastAdmin.into());
    }
    conn.execute("DELETE FROM users WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn create_session(db: &Db, user_id: &str, token_hash: &str) -> Result<()> {
    let now = Utc::now();
    let conn = db.conn();
    conn.execute(
        "DELETE FROM auth_sessions WHERE expires_at <= ?1",
        params![now],
    )?;
    conn.execute(
        "INSERT INTO auth_sessions (token_hash, user_id, created_at, expires_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            token_hash,
            user_id,
            now,
            now + Duration::days(SESSION_LIFETIME_DAYS)
        ],
    )?;
    Ok(())
}

pub fn user_for_session(db: &Db, token_hash: &str) -> Result<Option<User>> {
    db.conn()
        .query_row(
            "SELECT u.id, u.username, u.role, u.created_at FROM auth_sessions s \
             JOIN users u ON u.id = s.user_id \
             WHERE s.token_hash = ?1 AND s.expires_at > ?2",
            params![token_hash, Utc::now()],
            row_to_user,
        )
        .optional()
        .map_err(Into::into)
}

pub fn delete_session(db: &Db, token_hash: &str) -> Result<()> {
    db.conn().execute(
        "DELETE FROM auth_sessions WHERE token_hash = ?1",
        params![token_hash],
    )?;
    Ok(())
}

fn admin_count(conn: &rusqlite::Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM users WHERE role = 'admin'", [], |r| {
        r.get(0)
    })
}

fn row_to_user(row: &rusqlite::Row) -> rusqlite::Result<User> {
    let role: String = row.get(2)?;
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        role: Role::parse(&role),
        created_at: row.get(3)?,
    })
}
