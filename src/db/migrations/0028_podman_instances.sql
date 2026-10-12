-- The marker is no longer Valheim-specific: any game whose driver supports
-- Podman records its migrated instances here, keyed by the durable
-- `game_instances` identity that was already game-neutral.
ALTER TABLE valheim_podman_instances RENAME TO podman_instances;
