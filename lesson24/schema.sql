CREATE TABLE IF NOT EXISTS tasks (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 request_key TEXT NOT NULL UNIQUE,
 kind TEXT NOT NULL CHECK(kind IN ('uppercase','flaky')),
 payload TEXT NOT NULL,
 max_attempts INTEGER NOT NULL CHECK(max_attempts BETWEEN 1 AND 8),
 fail_until INTEGER NOT NULL CHECK(fail_until BETWEEN 0 AND 8),
 work_ms INTEGER NOT NULL CHECK(work_ms BETWEEN 0 AND 2000),
 delay_ms INTEGER NOT NULL CHECK(delay_ms BETWEEN 0 AND 60000),
 status TEXT NOT NULL CHECK(status IN ('queued','running','succeeded','failed','cancelled')),
 attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND max_attempts),
 run_token INTEGER NOT NULL DEFAULT 0 CHECK(run_token >= 0),
 next_run_at INTEGER NOT NULL CHECK(next_run_at >= 0),
 cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0,1)),
 result TEXT NOT NULL DEFAULT '',
 last_error TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS tasks_due ON tasks(status,next_run_at,id);
