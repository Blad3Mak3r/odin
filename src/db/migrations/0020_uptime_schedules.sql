-- A daily operating window shared by every supported game. Times are stored
-- as minutes after midnight in the host's local time zone so changing the
-- server's time zone does not require rewriting schedules.
CREATE TABLE uptime_schedules (
    instance_id  TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    enabled      INTEGER NOT NULL DEFAULT 0,
    start_minute INTEGER NOT NULL CHECK(start_minute >= 0 AND start_minute < 1440),
    stop_minute  INTEGER NOT NULL CHECK(stop_minute >= 0 AND stop_minute < 1440),
    CHECK(start_minute != stop_minute)
);
