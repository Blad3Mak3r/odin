-- One-time links that let an invited user choose their first password. Only
-- the SHA-256 digest of the token is stored. A row means the invitation has
-- not been accepted yet (it may have expired); accepting or any password
-- change removes it. At most one link per user, so resending replaces it.
CREATE TABLE password_links (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_email TEXT NOT NULL UNIQUE COLLATE NOCASE
        REFERENCES users(email) ON DELETE CASCADE ON UPDATE CASCADE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
