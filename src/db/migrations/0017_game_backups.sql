-- Key backup data by immutable game identity while preserving Valheim records.
CREATE TABLE backups_next (
 id TEXT NOT NULL, instance_name TEXT NOT NULL,
 instance_id TEXT NOT NULL REFERENCES game_instances(id) ON DELETE CASCADE,
 created_at TEXT NOT NULL, size_bytes INTEGER NOT NULL,
 remote_provider TEXT, remote_endpoint TEXT, remote_region TEXT, remote_bucket TEXT, remote_key TEXT,
 PRIMARY KEY(instance_id, id)
);
INSERT INTO backups_next SELECT id, instance_name, instance_id, created_at, size_bytes, remote_provider, remote_endpoint, remote_region, remote_bucket, remote_key FROM backups;
DROP TABLE backups;
ALTER TABLE backups_next RENAME TO backups;
CREATE TABLE backup_schedules_next (
 instance_name TEXT NOT NULL,
 instance_id TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
 interval_hours INTEGER NOT NULL CHECK(interval_hours > 0), retain_count INTEGER NOT NULL CHECK(retain_count > 0),
 enabled INTEGER NOT NULL DEFAULT 0, last_run_at TEXT
);
INSERT INTO backup_schedules_next SELECT instance_name, instance_id, interval_hours, retain_count, enabled, last_run_at FROM backup_schedules;
DROP TABLE backup_schedules;
ALTER TABLE backup_schedules_next RENAME TO backup_schedules;
CREATE TABLE backup_storage_configs_next (
 instance_name TEXT NOT NULL,
 instance_id TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
 provider TEXT NOT NULL CHECK(provider IN ('aws_s3', 'cloudflare_r2')),
 endpoint TEXT NOT NULL, region TEXT NOT NULL, bucket TEXT NOT NULL, prefix TEXT NOT NULL DEFAULT 'odin',
 access_key_id TEXT NOT NULL, secret_access_key TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1
);
INSERT INTO backup_storage_configs_next SELECT instance_name, instance_id, provider, endpoint, region, bucket, prefix, access_key_id, secret_access_key, enabled FROM backup_storage_configs;
DROP TABLE backup_storage_configs;
ALTER TABLE backup_storage_configs_next RENAME TO backup_storage_configs;
