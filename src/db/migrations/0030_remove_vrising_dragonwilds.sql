-- V Rising and RuneScape: Dragonwilds are no longer supported. Their identity
-- rows would otherwise fail to parse into a known game; deleting them cascades
-- to their configuration, backups, schedules and every other dependent record.
-- Files below the data directory are left untouched.
DELETE FROM game_instances WHERE game IN ('vrising', 'runescape-dragonwilds');
