-- Reserve a unique console port for 7D2D instances created before Odin
-- gained its loopback dashboard console. It follows the three game ports and
-- each later instance already advances its complete port group by ten.
UPDATE generic_game_instance_configs
SET admin_port = port + 3
WHERE instance_id IN (SELECT id FROM game_instances WHERE game = '7d2d')
  AND admin_port IS NULL;
