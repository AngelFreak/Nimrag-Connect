-- LEDGER (not derived; `reparse` never touches it). The stored blobs and their
-- `<sha256>.json` sidecars are the source of truth; `rebuild-ledger` can
-- recreate files/uploads from the sidecars. tokens exist only in this database.
-- DERIVED (deletable, rebuilt by `reparse`): fit_files, fit_messages, file_parse.
-- Derived rows contain no autoincrement ids and no timestamps of when they were made.

CREATE TABLE tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    hash         TEXT NOT NULL UNIQUE, -- SHA-256 hex of the token
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_used_at TEXT,
    revoked_at   TEXT
);

CREATE TABLE files (
    sha256          TEXT PRIMARY KEY CHECK (length(sha256) = 64),
    size            INTEGER NOT NULL,
    received_at     TEXT NOT NULL,
    source          TEXT NOT NULL CHECK (source IN ('watch', 'usb', 'import')),
    data_type       INTEGER,
    sub_type        INTEGER,
    file_index      INTEGER,
    file_number     INTEGER,
    watch_timestamp INTEGER
);

-- Every upload event, duplicates included, with the metadata as received.
-- device_id is informational (tokens are not rebuilt from sidecars).
CREATE TABLE uploads (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    sha256          TEXT NOT NULL REFERENCES files (sha256),
    device_id       INTEGER,
    device_name     TEXT,
    at              TEXT NOT NULL,
    source          TEXT NOT NULL,
    data_type       INTEGER,
    sub_type        INTEGER,
    file_index      INTEGER,
    file_number     INTEGER,
    watch_timestamp INTEGER,
    created         INTEGER NOT NULL CHECK (created IN (0, 1)),
    repeat_count    INTEGER NOT NULL DEFAULT 0, -- identical re-uploads (same device and metadata) folded into this row
    last_at         TEXT                        -- time of the latest folded repeat
);
CREATE INDEX uploads_sha256 ON uploads (sha256);

-- One row per FIT-typed (data_type 128) file that was parsed.
CREATE TABLE file_parse (
    sha256          TEXT PRIMARY KEY REFERENCES files (sha256),
    parser_version  INTEGER NOT NULL,
    profile_version INTEGER,           -- highest FIT profile_version in the chain headers
    status          TEXT NOT NULL CHECK (status IN ('ok', 'partial', 'error')),
    error           TEXT,
    crc_status      TEXT CHECK (crc_status IN ('ok', 'mismatch', 'missing')) -- first non-ok in chain, else ok; NULL if no FIT file decoded
) STRICT;

CREATE TABLE fit_files (
    sha256           TEXT NOT NULL REFERENCES files (sha256),
    chain_idx        INTEGER NOT NULL,
    header_size      INTEGER NOT NULL,
    protocol_version INTEGER NOT NULL,
    profile_version  INTEGER NOT NULL,
    data_size        INTEGER NOT NULL,
    header_crc       INTEGER,
    header_crc_ok    INTEGER NOT NULL,
    byte_offset      INTEGER NOT NULL, -- of this file's header within the upload
    byte_len         INTEGER NOT NULL,
    complete         INTEGER NOT NULL,
    crc_status       TEXT NOT NULL CHECK (crc_status IN ('ok', 'mismatch', 'missing')),
    crc_stored       INTEGER,
    crc_computed     INTEGER,
    crc_ok           INTEGER NOT NULL,
    message_count    INTEGER NOT NULL,
    developer_fields TEXT NOT NULL, -- JSON array of field descriptions
    PRIMARY KEY (sha256, chain_idx)
) STRICT, WITHOUT ROWID;

-- Raw messages. `fields` is a JSON object keyed by field number, in definition
-- order: {"<num>": {"t": base type, "be": true (only if big endian), "v": unscaled
-- raw value (null = invalid or non-finite float), "raw": hex of stored bytes}}.
-- No names, scaling or units: a profile change never rewrites this table.
-- Shape note: a one-element array is stored as a scalar, so "v" may be a scalar or
-- an array for the same field; readers must handle both (use "t" and "raw").
CREATE TABLE fit_messages (
    sha256       TEXT NOT NULL,
    chain_idx    INTEGER NOT NULL,
    msg_idx      INTEGER NOT NULL,
    global       INTEGER NOT NULL,
    local        INTEGER NOT NULL,
    timestamp    INTEGER, -- Message::timestamp()
    time_offset  INTEGER,
    byte_offset  INTEGER NOT NULL,
    fields       TEXT NOT NULL,
    developer    TEXT NOT NULL, -- JSON array [{developer_data_index, num, raw, v}]
    PRIMARY KEY (sha256, chain_idx, msg_idx),
    FOREIGN KEY (sha256, chain_idx) REFERENCES fit_files (sha256, chain_idx)
) STRICT, WITHOUT ROWID;
CREATE INDEX fit_messages_global_ts ON fit_messages (global, timestamp);
