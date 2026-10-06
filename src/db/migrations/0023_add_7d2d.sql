CREATE TABLE game_instances_next (
    id         TEXT PRIMARY KEY,
    game       TEXT NOT NULL CHECK (game IN ('valheim', 'rust', 'vrising', 'palworld', 'runescape-dragonwilds', '7d2d')),
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    tags       TEXT NOT NULL DEFAULT '[]',
    UNIQUE (game, name)
);
INSERT INTO game_instances_next (id, game, name, created_at, tags)
SELECT id, game, name, created_at, tags FROM game_instances;
DROP TABLE game_instances;
ALTER TABLE game_instances_next RENAME TO game_instances;
