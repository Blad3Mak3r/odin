ALTER TABLE rust_instance_configs ADD COLUMN rcon_port INTEGER;
ALTER TABLE rust_instance_configs ADD COLUMN rcon_password TEXT;

-- Existing instances receive enabled WebRCON endpoints that do not overlap
-- with any managed game or query port. New instances persist the same fields
-- as non-null values in their INSERT.
WITH RECURSIVE candidates(port) AS (
    VALUES(28015)
    UNION ALL
    SELECT port + 1 FROM candidates WHERE port < 65535
),
available AS (
    SELECT port, row_number() OVER (ORDER BY port) AS position
    FROM candidates
    WHERE port NOT IN (
        SELECT port FROM rust_instance_configs
        UNION SELECT query_port FROM rust_instance_configs
        UNION SELECT port FROM instances
        UNION SELECT port + 1 FROM instances
        UNION SELECT port + 2 FROM instances
    )
),
numbered_instances AS (
    SELECT instance_id, row_number() OVER (ORDER BY instance_id) AS position
    FROM rust_instance_configs
)
UPDATE rust_instance_configs
SET rcon_port = (
        SELECT available.port
        FROM available JOIN numbered_instances USING (position)
        WHERE numbered_instances.instance_id = rust_instance_configs.instance_id
    ),
    rcon_password = lower(hex(randomblob(16)));
