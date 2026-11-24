-- Offline edits (2026-11-24, docs/design/offline-edits.md).
--
-- Server side: `device_edits_applied` remembers each edit a phone sent (`POST /api/device/edits`)
-- with the answer it got, so a replay after a lost answer returns that answer and changes nothing.
-- Rows older than a month are pruned when a new one is written.
CREATE TABLE device_edits_applied (
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    op_id      TEXT NOT NULL,
    status     INTEGER NOT NULL,
    body       TEXT NOT NULL,
    applied_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, op_id)
) STRICT, WITHOUT ROWID;

-- Phone side (the embedded copy; unused on a server).
--
-- `edit_outbox`: one row per change made on the phone's copy, oldest (`seq`) first. A row is
-- written (`ready` = 0) before the handler runs and confirmed (`ready` = 1) when it succeeds, or
-- removed when it fails, so "the handler succeeded" always means "the row exists". `create_kind`
-- and `local_id` are set for a create (the new row's id on the phone).
CREATE TABLE edit_outbox (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    op_id       TEXT NOT NULL UNIQUE,
    method      TEXT NOT NULL,
    path        TEXT NOT NULL,
    body        TEXT NOT NULL,
    made_at     INTEGER NOT NULL,
    create_kind TEXT CHECK (create_kind IN ('workout', 'body')),
    local_id    INTEGER,
    ready       INTEGER NOT NULL DEFAULT 0 CHECK (ready IN (0, 1))
) STRICT;

-- `local id -> server id` of the rows created on the phone, per kind. `server_id` NULL: the
-- create was refused, so later edits of that row are dropped. Cleared when the state is pulled.
CREATE TABLE edit_id_map (
    kind      TEXT NOT NULL CHECK (kind IN ('workout', 'body')),
    local_id  INTEGER NOT NULL,
    server_id INTEGER,
    PRIMARY KEY (kind, local_id)
) STRICT, WITHOUT ROWID;

-- Changes the server could not apply, waiting to be shown once.
CREATE TABLE edit_dropped (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    method     TEXT NOT NULL,
    path       TEXT NOT NULL,
    reason     TEXT NOT NULL,
    dropped_at INTEGER NOT NULL
) STRICT;
