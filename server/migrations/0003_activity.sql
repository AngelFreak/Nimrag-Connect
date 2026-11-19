-- Activity entity tables: one row set per file (keyed by sha256), replaced when the
-- file is re-derived. All values come from the file bytes. SI units, unix seconds UTC.
-- Enum columns keep the raw code plus the profile's name when known.
CREATE TABLE activities (
    sha256          TEXT PRIMARY KEY REFERENCES files (sha256),
    activity_key    TEXT,    -- "<serial>:<time_created>" from file_id, NULL if either is absent
    serial          INTEGER,
    time_created    INTEGER,
    ts              INTEGER, -- activity.timestamp
    start_ts        INTEGER, -- first session start_time
    local_offset_s  INTEGER, -- activity.local_timestamp - activity.timestamp
    total_timer_s   REAL,
    num_sessions    INTEGER,
    sport           INTEGER, -- first session
    sport_name      TEXT,
    sub_sport       INTEGER,
    sub_sport_name  TEXT
) STRICT, WITHOUT ROWID;

-- Best-ranked file per activity (rank: ok desc, last_ts desc, msg_count desc, sha256 asc;
-- rank_key encodes the first three, sha256 asc breaks ties).
-- Files without an activity_key are their own activity.
CREATE VIEW activities_current AS
SELECT a.*
FROM activities a
JOIN sources s ON s.sha256 = a.sha256
WHERE NOT EXISTS (
    SELECT 1
    FROM activities b
    JOIN sources t ON t.sha256 = b.sha256
    WHERE COALESCE(b.activity_key, b.sha256) = COALESCE(a.activity_key, a.sha256)
      AND (t.rank_key > s.rank_key OR (t.rank_key = s.rank_key AND t.sha256 < s.sha256))
);

CREATE TABLE activity_sessions (
    sha256         TEXT NOT NULL REFERENCES files (sha256),
    idx            INTEGER NOT NULL, -- ordinal among session messages
    start_ts       INTEGER,
    sport          INTEGER,
    sport_name     TEXT,
    sub_sport      INTEGER,
    sub_sport_name TEXT,
    elapsed_s      REAL,
    timer_s        REAL,
    distance_m     REAL,
    calories       REAL,
    avg_hr         REAL,
    max_hr         REAL,
    avg_speed      REAL, -- m/s, enhanced_* preferred
    max_speed      REAL,
    ascent_m       REAL,
    descent_m      REAL,
    avg_cadence    REAL,
    aerobic_te     REAL,
    anaerobic_te   REAL,
    training_load  REAL, -- session.training_load_peak
    PRIMARY KEY (sha256, idx)
) STRICT, WITHOUT ROWID;

CREATE TABLE activity_laps (
    sha256         TEXT NOT NULL REFERENCES files (sha256),
    idx            INTEGER NOT NULL,
    start_ts       INTEGER,
    sport          INTEGER,
    sport_name     TEXT,
    sub_sport      INTEGER,
    sub_sport_name TEXT,
    elapsed_s      REAL,
    timer_s        REAL,
    distance_m     REAL,
    calories       REAL,
    avg_hr         REAL,
    max_hr         REAL,
    avg_speed      REAL,
    max_speed      REAL,
    ascent_m       REAL,
    descent_m      REAL,
    avg_cadence    REAL,
    PRIMARY KEY (sha256, idx)
) STRICT, WITHOUT ROWID;

CREATE TABLE activity_records (
    sha256        TEXT NOT NULL REFERENCES files (sha256),
    idx           INTEGER NOT NULL, -- ordinal among record messages
    ts            INTEGER NOT NULL,
    lat           REAL,             -- degrees (semicircles * 180 / 2^31)
    lon           REAL,
    hr            REAL,
    speed         REAL,             -- m/s, enhanced_speed preferred
    altitude_m    REAL,
    distance_m    REAL,
    cadence       REAL,
    temperature_c REAL,
    PRIMARY KEY (sha256, idx)
) STRICT, WITHOUT ROWID;

CREATE TABLE strength_sets (
    sha256                TEXT NOT NULL REFERENCES files (sha256),
    idx                   INTEGER NOT NULL,
    ts                    INTEGER,
    start_ts              INTEGER,
    duration_s            REAL,
    set_type              INTEGER,
    set_type_name         TEXT,
    reps                  REAL,
    weight_kg             REAL,    -- NULL when invalid, never 0
    category              INTEGER, -- first valid element
    category_name         TEXT,
    category_subtype      INTEGER, -- first valid element
    category_all          TEXT,    -- JSON list when more than one valid category
    category_subtype_all  TEXT,    -- JSON list when more than one valid subtype
    wkt_step_index        INTEGER,
    PRIMARY KEY (sha256, idx)
) STRICT, WITHOUT ROWID;

CREATE TABLE exercise_titles (
    sha256        TEXT NOT NULL REFERENCES files (sha256),
    idx           INTEGER NOT NULL,
    category      INTEGER,
    category_name TEXT,
    subtype       INTEGER,
    name          TEXT,
    PRIMARY KEY (sha256, idx)
) STRICT, WITHOUT ROWID;

CREATE TABLE activity_time_in_zone (
    sha256     TEXT NOT NULL REFERENCES files (sha256),
    idx        INTEGER NOT NULL,
    ref_mesg   INTEGER,
    ref_index  INTEGER,
    hr_zone_s  TEXT NOT NULL, -- JSON array of seconds per HR zone, null for invalid elements
    PRIMARY KEY (sha256, idx)
) STRICT, WITHOUT ROWID;

-- firstbeat_results (140): only fields the local profile resolves.
-- recovery_time_physical_activity_raw = firstbeat recovery_time_physical_activity as written (no unit claimed).
CREATE TABLE activity_metrics (
    sha256             TEXT PRIMARY KEY REFERENCES files (sha256),
    maximal_met        REAL,
    aerobic_te         REAL,
    anaerobic_te       REAL,
    training_load_peak REAL,
    epoc               REAL,
    recovery_time_physical_activity_raw       REAL
) STRICT, WITHOUT ROWID;
