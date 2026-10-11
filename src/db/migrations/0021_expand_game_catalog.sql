-- game_instances was introduced while Odin only supported Valheim and Rust.
-- Rebuild it so game identifiers remain constrained but can represent every
-- driver shipped by this binary. Foreign-key enforcement is disabled by the
-- migration runner while this table is replaced.
CREATE TABLE game_instances_next (
    id         TEXT PRIMARY KEY,
    game       TEXT NOT NULL CHECK (game IN ('valheim', 'rust', 'vrising', 'palworld', 'runescape-dragonwilds')),
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    tags       TEXT NOT NULL DEFAULT '[]',
    UNIQUE (game, name)
);
INSERT INTO game_instances_next (id, game, name, created_at, tags)
SELECT id, game, name, created_at, tags FROM game_instances;
DROP TABLE game_instances;
ALTER TABLE game_instances_next RENAME TO game_instances;

-- Config that is shared by the initially generic game drivers. Game-specific
-- settings live in config_json, which lets the dashboard render a safe,
-- driver-declared form without a schema migration for every game release.
CREATE TABLE generic_game_instance_configs (
    instance_id     TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    port            INTEGER NOT NULL,
    query_port      INTEGER,
    admin_port      INTEGER,
    config_json     TEXT NOT NULL DEFAULT '{}',
    auto_restart    INTEGER NOT NULL DEFAULT 0,
    pid             INTEGER,
    pid_started_at  INTEGER,
    last_started_at TEXT,
    last_stopped_at TEXT
);
