CREATE TABLE instances (
    name              TEXT PRIMARY KEY,
    port              INTEGER NOT NULL,
    world_name        TEXT NOT NULL,
    password          TEXT,
    public            INTEGER NOT NULL,
    created_at        TEXT NOT NULL,
    last_started_at   TEXT,
    last_stopped_at   TEXT,
    tmux_session      TEXT NOT NULL,
    bepinex_installed INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE installed_mods (
    instance_name TEXT NOT NULL REFERENCES instances(name) ON DELETE CASCADE,
    mod_id        TEXT NOT NULL,
    version       TEXT NOT NULL,
    installed_at  TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (instance_name, mod_id)
);

CREATE TABLE global_mods (
    mod_id     TEXT PRIMARY KEY,
    version    TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE access_list_entries (
    instance_name TEXT NOT NULL REFERENCES instances(name) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('admin', 'banned', 'permitted')),
    steam_id      TEXT NOT NULL,
    PRIMARY KEY (instance_name, kind, steam_id)
);

CREATE TABLE backups (
    id            TEXT NOT NULL,
    instance_name TEXT NOT NULL REFERENCES instances(name) ON DELETE CASCADE,
    created_at    TEXT NOT NULL,
    size_bytes    INTEGER NOT NULL,
    PRIMARY KEY (instance_name, id)
);

CREATE TABLE activity_events (
    id       TEXT PRIMARY KEY,
    at       TEXT NOT NULL,
    instance TEXT,
    kind     TEXT NOT NULL,
    payload  TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_activity_events_at ON activity_events(at);

CREATE TABLE cache (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);
