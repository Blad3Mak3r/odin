-- Dashboard accounts and their login sessions. The email is the primary key
-- (stored lowercase); the username is an optional second handle, unique when
-- present. Both match case-insensitively. `token_hash` is the SHA-256 of the opaque cookie value,
-- so a leaked database cannot be replayed as a login.
CREATE TABLE users (
    email         TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
    username      TEXT UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('admin', 'viewer')),
    created_at    TEXT NOT NULL
);

CREATE TABLE auth_sessions (
    token_hash TEXT PRIMARY KEY,
    user_email TEXT NOT NULL REFERENCES users(email) ON DELETE CASCADE ON UPDATE CASCADE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX idx_auth_sessions_user ON auth_sessions(user_email);
