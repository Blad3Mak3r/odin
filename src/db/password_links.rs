//! One-time invitation links. Only SHA-256 digests of the tokens are stored;
//! generating and hashing them lives in `web::auth`.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{OptionalExtension, params};

use super::Db;

/// How long an invitation link works.
pub const VALID_FOR: Duration = Duration::hours(72);

/// Stores the link for `email`, replacing any earlier one, and returns when
/// it expires.
pub fn issue(db: &Db, email: &str, token_hash: &str, valid_for: Duration) -> Result<DateTime<Utc>> {
    let now = Utc::now();
    let expires_at = now + valid_for;
    let conn = db.conn();
    conn.execute(
        "DELETE FROM password_links WHERE user_email = ?1",
        params![email],
    )?;
    conn.execute(
        "INSERT INTO password_links (token_hash, user_email, created_at, expires_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![token_hash, email, now, expires_at],
    )?;
    Ok(expires_at)
}

/// The account a live link belongs to, without using it up.
pub fn peek(db: &Db, token_hash: &str) -> Result<Option<String>> {
    db.conn()
        .query_row(
            "SELECT user_email FROM password_links \
             WHERE token_hash = ?1 AND expires_at > ?2 \
               AND EXISTS (SELECT 1 FROM users WHERE email = user_email AND disabled = 0)",
            params![token_hash, Utc::now()],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

/// Uses a live link up and returns its account. Atomic, so a link cannot be
/// redeemed twice.
pub fn consume(db: &Db, token_hash: &str) -> Result<Option<String>> {
    db.conn()
        .query_row(
            "DELETE FROM password_links \
             WHERE token_hash = ?1 AND expires_at > ?2 \
               AND EXISTS (SELECT 1 FROM users WHERE email = user_email AND disabled = 0) \
             RETURNING user_email",
            params![token_hash, Utc::now()],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

/// Whether `email` still has an unaccepted invitation, live or expired.
pub fn pending(db: &Db, email: &str) -> Result<bool> {
    db.conn()
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM password_links WHERE user_email = ?1)",
            params![email],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::users::{self, Role};
    use crate::paths::Paths;

    fn temp_db() -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-password-links-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        })
        .unwrap()
    }

    fn add_user(db: &Db, email: &str) {
        users::create(db, email, None, "hash", Role::Viewer, false).unwrap();
    }

    #[test]
    fn a_link_can_be_used_exactly_once() {
        let db = temp_db();
        add_user(&db, "ada@example.com");
        issue(&db, "ada@example.com", "t1", VALID_FOR).unwrap();

        assert_eq!(peek(&db, "t1").unwrap().as_deref(), Some("ada@example.com"));
        assert_eq!(peek(&db, "t1").unwrap().as_deref(), Some("ada@example.com"));
        assert_eq!(
            consume(&db, "t1").unwrap().as_deref(),
            Some("ada@example.com")
        );
        assert_eq!(consume(&db, "t1").unwrap(), None);
        assert_eq!(peek(&db, "t1").unwrap(), None);
        assert!(!pending(&db, "ada@example.com").unwrap());
    }

    #[test]
    fn issuing_again_replaces_the_previous_link() {
        let db = temp_db();
        add_user(&db, "ada@example.com");
        issue(&db, "ada@example.com", "old", VALID_FOR).unwrap();
        issue(&db, "ada@example.com", "new", VALID_FOR).unwrap();

        assert_eq!(peek(&db, "old").unwrap(), None);
        assert!(peek(&db, "new").unwrap().is_some());
    }

    #[test]
    fn an_expired_link_is_refused_but_stays_pending() {
        let db = temp_db();
        add_user(&db, "ada@example.com");
        issue(&db, "ada@example.com", "late", Duration::seconds(-1)).unwrap();

        assert_eq!(peek(&db, "late").unwrap(), None);
        assert_eq!(consume(&db, "late").unwrap(), None);
        assert!(pending(&db, "ada@example.com").unwrap());
    }

    #[test]
    fn a_disabled_account_cannot_use_its_link() {
        let db = temp_db();
        add_user(&db, "ada@example.com");
        issue(&db, "ada@example.com", "t1", VALID_FOR).unwrap();
        users::set_disabled(&db, "ada@example.com", true).unwrap();

        assert_eq!(peek(&db, "t1").unwrap(), None);
        assert_eq!(consume(&db, "t1").unwrap(), None);
    }

    #[test]
    fn a_password_change_or_deleting_the_user_removes_the_link() {
        let db = temp_db();
        add_user(&db, "ada@example.com");
        add_user(&db, "bob@example.com");
        issue(&db, "ada@example.com", "a", VALID_FOR).unwrap();
        issue(&db, "bob@example.com", "b", VALID_FOR).unwrap();

        users::set_password(&db, "ada@example.com", "new", false).unwrap();
        users::delete(&db, "bob@example.com").unwrap();

        assert!(!pending(&db, "ada@example.com").unwrap());
        assert_eq!(peek(&db, "b").unwrap(), None);
        assert!(!pending(&db, "bob@example.com").unwrap());
    }

    #[test]
    fn users_report_when_their_invitation_expires() {
        let db = temp_db();
        add_user(&db, "ada@example.com");
        assert_eq!(
            users::find_by_email(&db, "ada@example.com")
                .unwrap()
                .unwrap()
                .invitation_expires_at,
            None
        );

        let expires = issue(&db, "ada@example.com", "t1", VALID_FOR).unwrap();
        let listed = users::list(&db).unwrap();
        assert_eq!(listed[0].invitation_expires_at, Some(expires));
    }
}
