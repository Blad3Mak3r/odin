CREATE TABLE game_instances (
    id         TEXT PRIMARY KEY,
    game       TEXT NOT NULL CHECK (game IN ('valheim', 'rust')),
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (game, name)
);

-- Existing Valheim rows retain their directories and all their dependent
-- records. This index gives them the durable cross-game identity used by the
-- new API without rewriting proven Valheim tables in place.
INSERT INTO game_instances (id, game, name, created_at)
SELECT lower(hex(randomblob(16))), 'valheim', name, created_at FROM instances;

CREATE TABLE rust_instance_configs (
    instance_id     TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    port            INTEGER NOT NULL,
    query_port      INTEGER NOT NULL,
    hostname        TEXT NOT NULL,
    level           TEXT NOT NULL,
    seed            INTEGER NOT NULL,
    world_size      INTEGER NOT NULL,
    max_players     INTEGER NOT NULL,
    pid             INTEGER,
    pid_started_at  INTEGER,
    last_started_at TEXT,
    last_stopped_at TEXT
);
