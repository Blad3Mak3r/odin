CREATE TABLE backup_schedules (
    instance_name  TEXT PRIMARY KEY REFERENCES instances(name) ON DELETE CASCADE,
    interval_hours INTEGER NOT NULL,
    retain_count   INTEGER NOT NULL,
    enabled        INTEGER NOT NULL DEFAULT 0,
    last_run_at    TEXT
);
