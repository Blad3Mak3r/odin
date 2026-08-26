ALTER TABLE instances ADD COLUMN pid INTEGER;
ALTER TABLE instances ADD COLUMN pid_started_at INTEGER;
ALTER TABLE instances DROP COLUMN tmux_session;
