-- Capture file-owned settings before the application-level migration writes
-- them to each instance. The application drops these staging tables only
-- after every file has been written successfully.
CREATE TABLE rust_file_config_migrations (
    instance_id TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    hostname    TEXT NOT NULL,
    level       TEXT NOT NULL,
    seed        INTEGER NOT NULL,
    world_size  INTEGER NOT NULL,
    max_players INTEGER NOT NULL
);

INSERT INTO rust_file_config_migrations
    (instance_id, hostname, level, seed, world_size, max_players)
SELECT instance_id, hostname, level, seed, world_size, max_players
FROM rust_instance_configs;

CREATE TABLE generic_config_json_migrations (
    instance_id TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    config_json TEXT NOT NULL
);

INSERT INTO generic_config_json_migrations (instance_id, config_json)
SELECT instance_id, config_json
FROM generic_game_instance_configs
WHERE trim(config_json) NOT IN ('', '{}', 'null');

ALTER TABLE valheim_instance_configs ADD COLUMN save_interval INTEGER;
ALTER TABLE valheim_instance_configs ADD COLUMN backups INTEGER;
ALTER TABLE valheim_instance_configs ADD COLUMN backup_short INTEGER;
ALTER TABLE valheim_instance_configs ADD COLUMN backup_long INTEGER;
ALTER TABLE valheim_instance_configs ADD COLUMN crossplay INTEGER NOT NULL DEFAULT 0;
ALTER TABLE valheim_instance_configs ADD COLUMN playfab_instance_id TEXT;
ALTER TABLE valheim_instance_configs ADD COLUMN preset TEXT;
ALTER TABLE valheim_instance_configs ADD COLUMN modifiers_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE valheim_instance_configs ADD COLUMN set_keys_json TEXT NOT NULL DEFAULT '[]';
