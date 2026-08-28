CREATE TABLE jobs (
    id             TEXT PRIMARY KEY,
    kind           TEXT NOT NULL,
    kind_payload   TEXT NOT NULL,
    status         TEXT NOT NULL,
    status_payload TEXT NOT NULL,
    started_at     TEXT NOT NULL,
    log            TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX idx_jobs_started_at ON jobs(started_at);
