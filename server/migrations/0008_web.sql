-- Web interface: owner account, sessions, settings and owner-entered data.
-- None of these tables is derived: `reparse` never touches them.

-- One owner. The row exists after `user set-password`; the hash is argon2id (PHC string).
CREATE TABLE owner (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    password_hash TEXT NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

-- Browser sessions. The cookie holds 32 random bytes (hex); only the SHA-256 is stored.
CREATE TABLE sessions (
    id_hash    TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- key/value settings: timezone, step_goal, intensity_goal_min, home_cards (JSON),
-- sample_data ("1" when `dev-seed` filled the database).
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT, WITHOUT ROWID;

-- Edits to strength sets. Derived tables are rebuilt from the original files by
-- `reparse`, so edits live here, keyed by the activity's stable identity
-- (activity_key "<serial>:<time_created>" when the file has one, else its sha256)
-- and the set's ordinal in the file. `fields` is a JSON object holding only the
-- overridden fields: {"reps": 10, "weight_kg": null, "exercise": "Goblet squat"}.
CREATE TABLE strength_set_overrides (
    activity_ident TEXT NOT NULL,
    set_idx        INTEGER NOT NULL,
    fields         TEXT NOT NULL,
    updated_at     INTEGER NOT NULL,
    PRIMARY KEY (activity_ident, set_idx)
) STRICT, WITHOUT ROWID;

-- Manual body measurements (owner data).
CREATE TABLE body_measurements (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    measured_at  INTEGER NOT NULL, -- unix seconds UTC
    weight_kg    REAL,
    body_fat_pct REAL,
    waist_cm     REAL,
    created_at   INTEGER NOT NULL
) STRICT;
CREATE INDEX body_measurements_at ON body_measurements (measured_at);
