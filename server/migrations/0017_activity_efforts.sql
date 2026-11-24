-- Personal records: best efforts per activity (the fastest time over a given distance inside one
-- activity), computed from `activity_records` the first time they are asked for. Both tables are
-- entity tables (keyed by sha256), so re-deriving a file clears them and the next request
-- computes them again. `activity_efforts_done` says the file was computed, so a file that has no
-- effort at all is not computed twice. No foreign key to `files`: a sample activity (dev-seed) has
-- no file row, and a cache must not stop an answer.
CREATE TABLE activity_efforts_done (
    sha256 TEXT PRIMARY KEY
) STRICT, WITHOUT ROWID;

CREATE TABLE activity_efforts (
    sha256  TEXT NOT NULL,
    effort  TEXT NOT NULL, -- "400m", "1k", "1mi", "5k", "10k", "half", "marathon", "20k", "40k"
    seconds REAL NOT NULL,
    PRIMARY KEY (sha256, effort)
) STRICT, WITHOUT ROWID;
