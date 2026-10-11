-- The generic identity owns Valheim configuration from this point on. The
-- legacy instances table stays temporarily as the parent of unconverted
-- Valheim-only child tables; later migrations move those rows one domain at
-- a time without changing their files or names.
CREATE TABLE valheim_instance_configs (
    instance_id       TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    port              INTEGER NOT NULL,
    world_name        TEXT NOT NULL,
    password          TEXT,
    public            INTEGER NOT NULL,
    last_started_at   TEXT,
    last_stopped_at   TEXT,
    pid               INTEGER,
    pid_started_at    INTEGER,
    bepinex_installed INTEGER NOT NULL DEFAULT 0,
    auto_restart      INTEGER NOT NULL DEFAULT 0,
    bepinex_version   TEXT
);

INSERT INTO valheim_instance_configs
SELECT g.id, i.port, i.world_name, i.password, i.public, i.last_started_at,
       i.last_stopped_at, i.pid, i.pid_started_at, i.bepinex_installed,
       i.auto_restart, i.bepinex_version
FROM instances i
JOIN game_instances g ON g.game = 'valheim' AND g.name = i.name;
