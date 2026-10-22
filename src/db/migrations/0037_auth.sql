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

-- `id` is the public handle shown in the session list; `token_hash` stays
-- secret. A session slides: `expires_at` moves forward as it is used, up to
-- an absolute cap measured from `created_at` (enforced in code).
CREATE TABLE auth_sessions (
    token_hash   TEXT PRIMARY KEY,
    id           TEXT NOT NULL UNIQUE,
    user_email   TEXT NOT NULL REFERENCES users(email) ON DELETE CASCADE ON UPDATE CASCADE,
    created_at   TEXT NOT NULL,
    last_used_at TEXT NOT NULL,
    expires_at   TEXT NOT NULL,
    remember     INTEGER NOT NULL,
    ip           TEXT,
    user_agent   TEXT
);
CREATE INDEX idx_auth_sessions_user ON auth_sessions(user_email);
