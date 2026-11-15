CREATE TABLE IF NOT EXISTS users (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS points_accounts (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    points  INTEGER NOT NULL CHECK (points >= 0)
);

CREATE TABLE IF NOT EXISTS idempotency_records (
    idempotency_key TEXT PRIMARY KEY NOT NULL,
    request_name    TEXT NOT NULL,
    request_points  INTEGER NOT NULL,
    user_id         INTEGER NOT NULL REFERENCES users(id),
    response_name   TEXT NOT NULL,
    response_points INTEGER NOT NULL
);
