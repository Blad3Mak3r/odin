-- Every instance runs in a Podman container, so the table no longer marks
-- which instances were migrated; it only records whether an operator last
-- left an instance started. Rows are created the first time that is set.
CREATE TABLE instance_runtime_state (
    instance_id     TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    desired_running INTEGER NOT NULL DEFAULT 0
);
INSERT INTO instance_runtime_state (instance_id, desired_running)
SELECT instance_id, desired_running FROM podman_instances;
DROP TABLE podman_instances;
