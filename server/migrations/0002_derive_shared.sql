-- Shared by every derived parser. All values come from the file bytes.
-- Rank "A outranks B": ok desc, last_ts desc, msg_count desc, sha256 asc. rank_key
-- encodes the first three; src_id (derived from the sha256 prefix) breaks ties.
CREATE TABLE sources (
    sha256       TEXT PRIMARY KEY REFERENCES files (sha256),
    src_id       INTEGER NOT NULL UNIQUE, -- first 8 bytes of sha256 as a positive i64
    ok           INTEGER NOT NULL,        -- all chain files complete with a good CRC
    last_ts      INTEGER,
    msg_count    INTEGER NOT NULL,
    serial       INTEGER,
    time_created INTEGER,
    file_type    INTEGER,
    rank_key     TEXT NOT NULL
) STRICT;

CREATE TABLE file_derive (
    sha256         TEXT PRIMARY KEY REFERENCES files (sha256),
    derive_version INTEGER NOT NULL,
    status         TEXT NOT NULL CHECK (status IN ('ok', 'partial', 'error', 'no_profile')),
    error          TEXT,
    skipped_rows   INTEGER NOT NULL
) STRICT;
