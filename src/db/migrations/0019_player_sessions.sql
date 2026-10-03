CREATE TABLE player_sessions (
    id          TEXT PRIMARY KEY,
    instance_id TEXT NOT NULL REFERENCES game_instances(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    steam_id    TEXT,
    joined_at   TEXT NOT NULL,
    left_at     TEXT
);
CREATE INDEX idx_player_sessions_instance_joined
    ON player_sessions(instance_id, joined_at DESC);
