ALTER TABLE global_mods RENAME TO global_mods_single_version;

CREATE TABLE global_mods (
    mod_id     TEXT NOT NULL,
    version    TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (mod_id, version)
);

INSERT INTO global_mods (mod_id, version, updated_at)
SELECT mod_id, version, updated_at FROM global_mods_single_version;

DROP TABLE global_mods_single_version;

ALTER TABLE installed_mods
ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;

-- The old store exposed one mutable payload per mod. If an update made an
-- instance's recorded version stale, every instance still used that newest
-- payload through the shared symlink. Preserve the version actually in use.
UPDATE installed_mods
SET version = (
    SELECT global_mods.version
    FROM global_mods
    WHERE global_mods.mod_id = installed_mods.mod_id
)
WHERE EXISTS (
    SELECT 1 FROM global_mods
    WHERE global_mods.mod_id = installed_mods.mod_id
);
