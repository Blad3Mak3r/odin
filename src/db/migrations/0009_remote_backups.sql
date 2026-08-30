CREATE TABLE backup_storage_configs (
    instance_name     TEXT PRIMARY KEY REFERENCES instances(name) ON DELETE CASCADE,
    provider          TEXT NOT NULL CHECK (provider IN ('aws_s3', 'cloudflare_r2')),
    endpoint          TEXT NOT NULL,
    region            TEXT NOT NULL,
    bucket            TEXT NOT NULL,
    prefix            TEXT NOT NULL DEFAULT 'odin',
    access_key_id     TEXT NOT NULL,
    secret_access_key TEXT NOT NULL,
    enabled           INTEGER NOT NULL DEFAULT 1
);

ALTER TABLE backups ADD COLUMN remote_provider TEXT
    CHECK (remote_provider IS NULL OR remote_provider IN ('aws_s3', 'cloudflare_r2'));
ALTER TABLE backups ADD COLUMN remote_endpoint TEXT;
ALTER TABLE backups ADD COLUMN remote_region TEXT;
ALTER TABLE backups ADD COLUMN remote_bucket TEXT;
ALTER TABLE backups ADD COLUMN remote_key TEXT;
