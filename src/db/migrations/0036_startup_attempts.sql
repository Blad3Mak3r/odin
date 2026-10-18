-- One row per attempt to start an instance's container: the phases it went
-- through and, when it failed, a structured reason. `events` and `failure`
-- are JSON owned by the Podman layer; only the newest few attempts per
-- instance are kept.
CREATE TABLE startup_attempts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    instance_id TEXT NOT NULL REFERENCES game_instances(id) ON DELETE CASCADE,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    outcome     TEXT NOT NULL CHECK (outcome IN ('running', 'succeeded', 'failed')),
    events      TEXT NOT NULL DEFAULT '[]',
    failure     TEXT
);
CREATE INDEX idx_startup_attempts_instance ON startup_attempts(instance_id, id DESC);
