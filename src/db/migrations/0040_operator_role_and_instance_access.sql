-- Adds the `operator` role, the account owner, and per-instance access for
-- viewers and operators. Widening the role CHECK means rebuilding `users`;
-- the runner turns foreign keys off for this migration so the sessions that
-- reference it survive the swap.
CREATE TABLE users_next (
    email         TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
    username      TEXT UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('admin', 'operator', 'viewer')),
    must_change_password INTEGER NOT NULL DEFAULT 0,
    disabled      INTEGER NOT NULL DEFAULT 0,
    last_login_at TEXT,
    created_at    TEXT NOT NULL,
    -- The account created at first-run setup. Other administrators cannot
    -- demote, disable, delete or reset it.
    is_owner      INTEGER NOT NULL DEFAULT 0
);
INSERT INTO users_next (email, username, password_hash, role, must_change_password, disabled, last_login_at, created_at)
SELECT email, username, password_hash, role, must_change_password, disabled, last_login_at, created_at FROM users;
DROP TABLE users;
ALTER TABLE users_next RENAME TO users;

-- Existing installs never recorded the owner: it is the oldest administrator.
UPDATE users SET is_owner = 1
WHERE email = (SELECT email FROM users WHERE role = 'admin' ORDER BY created_at, rowid LIMIT 1);
CREATE UNIQUE INDEX idx_users_single_owner ON users(is_owner) WHERE is_owner = 1;

-- The instances a viewer or operator may see. Administrators ignore it.
CREATE TABLE user_instance_access (
    user_email  TEXT NOT NULL COLLATE NOCASE REFERENCES users(email) ON DELETE CASCADE ON UPDATE CASCADE,
    instance_id TEXT NOT NULL REFERENCES game_instances(id) ON DELETE CASCADE,
    PRIMARY KEY (user_email, instance_id)
);
CREATE INDEX idx_user_instance_access_instance ON user_instance_access(instance_id);

-- Viewers could see every instance before this existed; keep that until an
-- administrator narrows it.
INSERT INTO user_instance_access (user_email, instance_id)
SELECT u.email, g.id FROM users u CROSS JOIN game_instances g WHERE u.role = 'viewer';
