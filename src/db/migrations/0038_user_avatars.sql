-- Profile pictures, kept apart from `users` so the image never travels with
-- the account row. The server stores one small WebP per user; `updated_at`
-- doubles as the cache-busting version the dashboard puts in the image URL.
CREATE TABLE user_avatars (
    user_email TEXT PRIMARY KEY NOT NULL COLLATE NOCASE
        REFERENCES users(email) ON DELETE CASCADE ON UPDATE CASCADE,
    webp BLOB NOT NULL,
    updated_at TEXT NOT NULL
);
