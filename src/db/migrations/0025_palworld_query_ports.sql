-- Palworld otherwise asks Steam to bind 27015 for every server process.
-- Existing instances predate the explicit query-port field. Allocate distinct
-- free ports for them, considering every configured game listener.
WITH RECURSIVE
unassigned AS MATERIALIZED (
    SELECT configs.instance_id, ROW_NUMBER() OVER (ORDER BY configs.instance_id) AS ordinal
    FROM generic_game_instance_configs AS configs
    JOIN game_instances ON game_instances.id = configs.instance_id
    WHERE game_instances.game = 'palworld'
      AND configs.query_port IS NULL
),
occupied(port) AS (
    SELECT port FROM instances
    UNION SELECT port + 1 FROM instances WHERE port < 65535
    UNION SELECT port + 2 FROM instances WHERE port < 65534
    UNION SELECT port FROM rust_instance_configs
    UNION SELECT query_port FROM rust_instance_configs
    UNION SELECT rcon_port FROM rust_instance_configs
    UNION SELECT port FROM generic_game_instance_configs
    UNION SELECT query_port FROM generic_game_instance_configs WHERE query_port IS NOT NULL
    UNION SELECT admin_port FROM generic_game_instance_configs WHERE admin_port IS NOT NULL
),
candidates(port) AS (
    SELECT 27015
    UNION ALL
    SELECT port + 1 FROM candidates WHERE port < 65535
),
available AS MATERIALIZED (
    SELECT candidates.port, ROW_NUMBER() OVER (ORDER BY candidates.port) AS ordinal
    FROM candidates
    WHERE NOT EXISTS (SELECT 1 FROM occupied WHERE occupied.port = candidates.port)
)
UPDATE generic_game_instance_configs
SET query_port = (SELECT port FROM available WHERE available.ordinal = (
    SELECT ordinal FROM unassigned
    WHERE unassigned.instance_id = generic_game_instance_configs.instance_id
))
WHERE instance_id IN (SELECT instance_id FROM unassigned);
