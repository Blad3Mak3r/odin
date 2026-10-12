-- Every game runs in a container, so Odin no longer records a host pid or its
-- start/stop times; these columns were never written again and are always NULL.
ALTER TABLE instances DROP COLUMN last_started_at;
ALTER TABLE instances DROP COLUMN last_stopped_at;
ALTER TABLE instances DROP COLUMN pid;
ALTER TABLE instances DROP COLUMN pid_started_at;
ALTER TABLE valheim_instance_configs DROP COLUMN last_started_at;
ALTER TABLE valheim_instance_configs DROP COLUMN last_stopped_at;
ALTER TABLE valheim_instance_configs DROP COLUMN pid;
ALTER TABLE valheim_instance_configs DROP COLUMN pid_started_at;
ALTER TABLE rust_instance_configs DROP COLUMN last_started_at;
ALTER TABLE rust_instance_configs DROP COLUMN last_stopped_at;
ALTER TABLE rust_instance_configs DROP COLUMN pid;
ALTER TABLE rust_instance_configs DROP COLUMN pid_started_at;
ALTER TABLE generic_game_instance_configs DROP COLUMN last_started_at;
ALTER TABLE generic_game_instance_configs DROP COLUMN last_stopped_at;
ALTER TABLE generic_game_instance_configs DROP COLUMN pid;
ALTER TABLE generic_game_instance_configs DROP COLUMN pid_started_at;
