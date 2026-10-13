CREATE TABLE ark_clusters (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    cluster_id TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE TABLE ark_cluster_members (
    instance_id TEXT PRIMARY KEY REFERENCES game_instances(id) ON DELETE CASCADE,
    cluster_id TEXT NOT NULL REFERENCES ark_clusters(id) ON DELETE CASCADE
);

CREATE INDEX idx_ark_cluster_members_cluster_id ON ark_cluster_members(cluster_id);
