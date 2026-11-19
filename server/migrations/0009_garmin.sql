-- Forward mode (Stage 2g): upload activity files to the owner's Garmin Connect account.
-- Not derived: `reparse` never touches these tables.

-- One row once Forward is switched on (a login or a token import succeeded); logout deletes it.
-- The DI OAuth2 tokens are secrets: never logged, never returned by the API. The password is never stored.
CREATE TABLE garmin_account (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    di_token         TEXT NOT NULL,     -- access token (JWT); '' after the login expired
    di_refresh_token TEXT NOT NULL,     -- '' after the login expired
    di_client_id     TEXT NOT NULL,
    status           TEXT NOT NULL CHECK (status IN ('connected', 'paused', 'needs_login')),
    last_error       TEXT,
    enabled_at       INTEGER NOT NULL,  -- unix seconds: when Forward was switched on
    updated_at       INTEGER NOT NULL
) STRICT;

-- Upload queue: one row per activity file (by content hash).
-- status: pending (waiting, maybe backing off), done (uploaded, or Garmin already had it),
-- failed (Garmin refused it, or 10 tries failed: needs the owner, `retry` re-queues).
CREATE TABLE garmin_uploads (
    file_sha256        TEXT PRIMARY KEY REFERENCES files (sha256),
    status             TEXT NOT NULL CHECK (status IN ('pending', 'done', 'failed')),
    attempts           INTEGER NOT NULL DEFAULT 0,
    next_attempt_at    INTEGER NOT NULL,  -- unix seconds
    garmin_activity_id INTEGER,
    last_error         TEXT,
    created_at         INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
CREATE INDEX garmin_uploads_due ON garmin_uploads (status, next_attempt_at);
