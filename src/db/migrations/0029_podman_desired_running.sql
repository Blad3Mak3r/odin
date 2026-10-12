-- Operator intent for a containerized instance: set when it starts and
-- cleared when an operator stops it. A container that is found dead while
-- this is set was not stopped on purpose, which is what lets Odin recover it
-- without a host pid to fingerprint. Instances that were already running
-- before this column existed are tracked from their next start.
ALTER TABLE podman_instances ADD COLUMN desired_running INTEGER NOT NULL DEFAULT 0;
