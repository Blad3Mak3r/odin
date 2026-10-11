ALTER TABLE activity_events ADD COLUMN game TEXT CHECK (game IS NULL OR game IN ('valheim', 'rust'));
UPDATE activity_events SET game = 'valheim' WHERE game IS NULL;
