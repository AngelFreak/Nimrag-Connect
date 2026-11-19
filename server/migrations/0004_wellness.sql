-- Wellness: sample tables (natural key + src = sources.src_id, rank-based upsert,
-- cleared and rebuilt by reparse) and two per-file entity tables.
-- No foreign key on src: sources rows are replaced per file while samples remain.

CREATE TABLE mon_hr (
    ts  INTEGER PRIMARY KEY,
    bpm REAL NOT NULL,
    src INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE mon_activity (
    ts              INTEGER NOT NULL,
    activity_type   INTEGER NOT NULL,
    steps           REAL,
    distance_m      REAL,
    active_calories REAL,
    active_time_s   REAL,
    duration_min    REAL,
    intensity       REAL,
    src             INTEGER NOT NULL,
    PRIMARY KEY (ts, activity_type)
) STRICT, WITHOUT ROWID;

CREATE TABLE mon_extra (
    ts           INTEGER PRIMARY KEY,
    ascent_total_m     REAL,
    descent_total_m    REAL,
    moderate_total_min REAL,
    vigorous_total_min REAL,
    src          INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE stress (
    ts                  INTEGER PRIMARY KEY,
    stress              REAL,
    stress_raw          REAL,
    body_battery        REAL,
    body_battery_status INTEGER,
    src                 INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE respiration (
    ts       INTEGER PRIMARY KEY,
    brpm     REAL,
    brpm_raw REAL,
    src      INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE spo2 (
    ts         INTEGER PRIMARY KEY,
    percent    REAL,
    confidence REAL,
    mode       INTEGER,
    src        INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE resting_hr (
    ts              INTEGER PRIMARY KEY,
    rhr             REAL,
    current_day_rhr REAL,
    src             INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE sleep_levels (
    ts         INTEGER PRIMARY KEY,
    level      INTEGER NOT NULL,
    level_name TEXT,
    src        INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE hrv_values (
    ts       INTEGER PRIMARY KEY,
    rmssd_ms REAL NOT NULL,
    src      INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE hrv_summary (
    ts                      INTEGER PRIMARY KEY,
    weekly_avg              REAL,
    last_night_avg          REAL,
    last_night_5min_high    REAL,
    baseline_low_upper      REAL,
    baseline_balanced_lower REAL,
    baseline_balanced_upper REAL,
    status                  INTEGER,
    src                     INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE training_load (
    ts           INTEGER PRIMARY KEY,
    acute        REAL,
    chronic      REAL,
    acwr         REAL,
    acwr_percent REAL,
    status       INTEGER,
    src          INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE training_status (
    ts              INTEGER PRIMARY KEY,
    status          INTEGER,
    weekly_load_sum REAL,
    load_min        REAL,
    load_max        REAL,
    fitness_trend   INTEGER,
    load_trend      INTEGER,
    src             INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE training_balance (
    ts                      INTEGER PRIMARY KEY,
    aerobic_low             REAL,
    aerobic_high            REAL,
    anaerobic               REAL,
    aerobic_low_target_min  REAL,
    aerobic_low_target_max  REAL,
    aerobic_high_target_min REAL,
    aerobic_high_target_max REAL,
    anaerobic_target_min    REAL,
    anaerobic_target_max    REAL,
    src                     INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE recovery_time (
    start_ts INTEGER PRIMARY KEY,
    minutes  REAL NOT NULL,
    src      INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE vo2max (
    ts           INTEGER PRIMARY KEY,
    maximal_met  REAL NOT NULL, -- as written by the watch; no MET x 3.5 conversion
    sport        INTEGER,
    src          INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- Entity tables: one row set per file, replaced with the file.
CREATE TABLE mon_info (
    sha256         TEXT PRIMARY KEY REFERENCES files (sha256),
    local_offset_s REAL,
    rmr_kcal_day   REAL,
    steps_goal     REAL
) STRICT, WITHOUT ROWID;

CREATE TABLE sleep_sessions (
    sha256           TEXT PRIMARY KEY REFERENCES files (sha256),
    start_ts         INTEGER,
    end_ts           INTEGER,
    overall_score    REAL,
    quality_score    REAL,
    duration_score   REAL,
    recovery_score   REAL,
    deep_score       REAL,
    light_score      REAL,
    rem_score        REAL,
    awakenings_count REAL,
    avg_stress       REAL
) STRICT, WITHOUT ROWID;

-- Best-ranked file per sleep (natural key start_ts; a session without a start
-- time is its own key). Same rank as sources: ok, last_ts, msg_count, sha256.
CREATE VIEW sleep_sessions_current AS
SELECT sha256, start_ts, end_ts, overall_score, quality_score, duration_score,
       recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress
FROM (
    SELECT s.*,
           ROW_NUMBER() OVER (
               PARTITION BY COALESCE(CAST(s.start_ts AS TEXT), s.sha256)
               ORDER BY o.rank_key DESC, o.sha256 ASC
           ) AS rn
    FROM sleep_sessions s JOIN sources o ON o.sha256 = s.sha256
)
WHERE rn = 1;
