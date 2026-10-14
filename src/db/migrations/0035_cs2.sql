CREATE TABLE game_instances_next (
    id         TEXT PRIMARY KEY,
    game       TEXT NOT NULL CHECK (game IN ('valheim', 'rust', 'palworld', '7d2d', 'ark-asa', 'cs2')),
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    tags       TEXT NOT NULL DEFAULT '[]',
    UNIQUE (game, name)
);
INSERT INTO game_instances_next (id, game, name, created_at, tags)
SELECT id, game, name, created_at, tags FROM game_instances;
DROP TABLE game_instances;
ALTER TABLE game_instances_next RENAME TO game_instances;

CREATE TABLE activity_events_next (
    id          TEXT PRIMARY KEY,
    at          TEXT NOT NULL,
    instance    TEXT,
    kind        TEXT NOT NULL,
    payload     TEXT NOT NULL DEFAULT '{}',
    instance_id TEXT REFERENCES game_instances(id) ON DELETE SET NULL,
    game        TEXT CHECK (game IS NULL OR game IN ('valheim', 'rust', 'palworld', '7d2d', 'ark-asa', 'cs2'))
);
INSERT INTO activity_events_next (id, at, instance, kind, payload, instance_id, game)
SELECT id, at, instance, kind, payload, instance_id, game FROM activity_events;
DROP TABLE activity_events;
ALTER TABLE activity_events_next RENAME TO activity_events;
CREATE INDEX idx_activity_events_at ON activity_events(at);
CREATE INDEX idx_activity_events_instance_id ON activity_events(instance_id);

CREATE TABLE cs2_launch_options (
    instance_id   TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    map           TEXT NOT NULL,
    game_mode     TEXT NOT NULL,
    max_players   INTEGER NOT NULL,
    gslt          TEXT,
    rcon_password TEXT NOT NULL
);
