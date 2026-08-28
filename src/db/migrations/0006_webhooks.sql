CREATE TABLE webhooks (
    id          TEXT PRIMARY KEY,
    url         TEXT NOT NULL,
    enabled     INTEGER NOT NULL DEFAULT 1,
    event_kinds TEXT NOT NULL DEFAULT '[]',
    created_at  TEXT NOT NULL
);
