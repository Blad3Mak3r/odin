CREATE TABLE resource_samples (
    instance_name TEXT REFERENCES instances(name) ON DELETE CASCADE,
    at            TEXT NOT NULL,
    cpu_percent   REAL NOT NULL,
    memory_bytes  INTEGER NOT NULL
);
CREATE INDEX idx_resource_samples_series_at ON resource_samples(instance_name, at);
