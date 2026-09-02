-- Keep the legacy name columns while every Valheim-owned record gains the
-- generic identity. Reads and public APIs remain name-based until each domain
-- can be switched independently without changing existing Valheim behavior.
ALTER TABLE installed_mods ADD COLUMN instance_id TEXT REFERENCES game_instances(id) ON DELETE CASCADE;
UPDATE installed_mods
SET instance_id = (
    SELECT id FROM game_instances
    WHERE game = 'valheim' AND name = installed_mods.instance_name
);
CREATE INDEX idx_installed_mods_instance_id ON installed_mods(instance_id);

ALTER TABLE access_list_entries ADD COLUMN instance_id TEXT REFERENCES game_instances(id) ON DELETE CASCADE;
UPDATE access_list_entries
SET instance_id = (
    SELECT id FROM game_instances
    WHERE game = 'valheim' AND name = access_list_entries.instance_name
);
CREATE INDEX idx_access_list_entries_instance_id ON access_list_entries(instance_id);

ALTER TABLE backups ADD COLUMN instance_id TEXT REFERENCES game_instances(id) ON DELETE CASCADE;
UPDATE backups
SET instance_id = (
    SELECT id FROM game_instances
    WHERE game = 'valheim' AND name = backups.instance_name
);
CREATE INDEX idx_backups_instance_id ON backups(instance_id);

ALTER TABLE backup_schedules ADD COLUMN instance_id TEXT REFERENCES game_instances(id) ON DELETE CASCADE;
UPDATE backup_schedules
SET instance_id = (
    SELECT id FROM game_instances
    WHERE game = 'valheim' AND name = backup_schedules.instance_name
);
CREATE INDEX idx_backup_schedules_instance_id ON backup_schedules(instance_id);

ALTER TABLE backup_storage_configs ADD COLUMN instance_id TEXT REFERENCES game_instances(id) ON DELETE CASCADE;
UPDATE backup_storage_configs
SET instance_id = (
    SELECT id FROM game_instances
    WHERE game = 'valheim' AND name = backup_storage_configs.instance_name
);
CREATE INDEX idx_backup_storage_configs_instance_id ON backup_storage_configs(instance_id);

ALTER TABLE resource_samples ADD COLUMN instance_id TEXT REFERENCES game_instances(id) ON DELETE CASCADE;
UPDATE resource_samples
SET instance_id = (
    SELECT id FROM game_instances
    WHERE game = 'valheim' AND name = resource_samples.instance_name
)
WHERE instance_name IS NOT NULL;
CREATE INDEX idx_resource_samples_instance_id_at ON resource_samples(instance_id, at);

ALTER TABLE activity_events ADD COLUMN instance_id TEXT REFERENCES game_instances(id) ON DELETE SET NULL;
UPDATE activity_events
SET instance_id = (
    SELECT id FROM game_instances
    WHERE game = 'valheim' AND name = activity_events.instance
)
WHERE instance IS NOT NULL;
CREATE INDEX idx_activity_events_instance_id ON activity_events(instance_id);
