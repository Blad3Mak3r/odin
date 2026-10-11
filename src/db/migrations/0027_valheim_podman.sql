-- The row is an explicit migration marker. Existing Valheim instances stay
-- direct until an operator migrates them; newly-created ones get a row.
CREATE TABLE valheim_podman_instances (
    instance_id TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    migrated_at TEXT NOT NULL
);
