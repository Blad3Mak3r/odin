-- Resource controls are common to every game driver. A missing row means
-- unlimited resources, preserving the behaviour of existing installations.
CREATE TABLE instance_resource_limits (
    instance_id      TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    cpu_percent      REAL,
    memory_max_bytes INTEGER,
    CHECK (cpu_percent IS NULL OR cpu_percent > 0),
    CHECK (memory_max_bytes IS NULL OR memory_max_bytes > 0)
);
