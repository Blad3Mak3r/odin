-- Palworld otherwise asks Steam to bind 27015 for every server process.
-- Existing instances predate the explicit query-port field. Give each one a
-- distinct, non-default Steam query port; newly-created instances use the
-- normal allocator and expose the setting in the dashboard.
WITH unassigned AS MATERIALIZED (
    SELECT configs.instance_id, ROW_NUMBER() OVER (ORDER BY configs.instance_id) AS ordinal
    FROM generic_game_instance_configs AS configs
    JOIN game_instances ON game_instances.id = configs.instance_id
    WHERE game_instances.game = 'palworld'
      AND configs.query_port IS NULL
)
UPDATE generic_game_instance_configs
SET query_port = 27100 + 10 * (
    SELECT ordinal - 1 FROM unassigned
    WHERE unassigned.instance_id = generic_game_instance_configs.instance_id
)
WHERE instance_id IN (SELECT instance_id FROM unassigned);
