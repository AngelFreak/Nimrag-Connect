-- Multi-user accounts (2026-11-20).
--
-- * `users` replaces the single `owner` row: the old owner becomes user `admin` (role admin, same
--   argon2id hash, 2FA not enrolled, so the first login enrols it). Sessions are cleared.
-- * Device tokens belong to a user (`tokens.user_id`; NULL = unassigned, as every existing token
--   is after this migration) and may carry a trusted-device credential (`device_secret_hash`).
-- * `file_owners` says whose file a sha256 is. Existing files have no owner row: they are
--   unassigned until the admin assigns the token that uploaded them. It is ledger data, so
--   `reparse` never touches it.
-- * Sample tables (natural key ts) get `user_id` in their primary key, so two users' samples at
--   the same second do not collide. 0 = unassigned (all existing rows). Rows are copied, not
--   rebuilt: the migration loses nothing.
-- * Owner data (settings, body measurements, strength edits, Garmin Forward) becomes per user;
--   existing rows get user_id 0 and the admin can adopt them for a user when assigning tokens.

CREATE TABLE users (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    username             TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (length(username) BETWEEN 3 AND 32),
    password_hash        TEXT NOT NULL,  -- argon2id PHC string
    role                 TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    totp_secret          TEXT,           -- base32 (RFC 4648), NULL until 2FA is enrolled
    totp_enabled         INTEGER NOT NULL DEFAULT 0 CHECK (totp_enabled IN (0, 1)),
    totp_last_step       INTEGER NOT NULL DEFAULT 0, -- last accepted 30 s step: a code is never accepted twice
    must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
    created_at           INTEGER NOT NULL,
    disabled_at          INTEGER
) STRICT;

-- The existing owner becomes the admin. The hash is copied byte for byte.
INSERT INTO users (username, password_hash, role, totp_enabled, must_change_password, created_at)
SELECT 'admin', password_hash, 'admin', 0, 0, updated_at FROM owner;
DROP TABLE owner;

-- Browser and app sessions. `token_id` ties an app session to its device token, so revoking the
-- token ends the session too.
DROP TABLE sessions;
CREATE TABLE sessions (
    id_hash    TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_id   INTEGER REFERENCES tokens (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
CREATE INDEX sessions_user ON sessions (user_id);

ALTER TABLE tokens ADD COLUMN user_id INTEGER REFERENCES users (id);
-- SHA-256 of the app's trusted-device secret (256 random bits); NULL for plain upload tokens.
ALTER TABLE tokens ADD COLUMN device_secret_hash TEXT;
CREATE INDEX tokens_user ON tokens (user_id);

CREATE TABLE file_owners (
    sha256  TEXT PRIMARY KEY REFERENCES files (sha256),
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX file_owners_user ON file_owners (user_id);

-- Per-user preferences (timezone, step_goal, intensity_goal_min, home_cards). `settings` keeps
-- only server-wide flags (sample_data). user_id 0 holds the pre-multi-user values.
CREATE TABLE user_settings (
    user_id INTEGER NOT NULL,
    key     TEXT NOT NULL,
    value   TEXT NOT NULL,
    PRIMARY KEY (user_id, key)
) STRICT, WITHOUT ROWID;
INSERT INTO user_settings (user_id, key, value)
SELECT 0, key, value FROM settings WHERE key IN ('timezone', 'step_goal', 'intensity_goal_min', 'home_cards');
DELETE FROM settings WHERE key IN ('timezone', 'step_goal', 'intensity_goal_min', 'home_cards');

ALTER TABLE body_measurements ADD COLUMN user_id INTEGER NOT NULL DEFAULT 0;
DROP INDEX body_measurements_at;
CREATE INDEX body_measurements_user_at ON body_measurements (user_id, measured_at);

ALTER TABLE strength_set_overrides RENAME TO strength_set_overrides_old;
CREATE TABLE strength_set_overrides (
    user_id        INTEGER NOT NULL DEFAULT 0,
    activity_ident TEXT NOT NULL,
    set_idx        INTEGER NOT NULL,
    fields         TEXT NOT NULL,
    updated_at     INTEGER NOT NULL,
    PRIMARY KEY (user_id, activity_ident, set_idx)
) STRICT, WITHOUT ROWID;
INSERT INTO strength_set_overrides (user_id, activity_ident, set_idx, fields, updated_at)
SELECT 0, activity_ident, set_idx, fields, updated_at FROM strength_set_overrides_old;
DROP TABLE strength_set_overrides_old;

-- Garmin Forward: one account per user (user_id 0 = the pre-multi-user account); each queued file
-- records whose account it goes to.
ALTER TABLE garmin_account RENAME TO garmin_account_old;
CREATE TABLE garmin_account (
    user_id          INTEGER PRIMARY KEY,
    di_token         TEXT NOT NULL,
    di_refresh_token TEXT NOT NULL,
    di_client_id     TEXT NOT NULL,
    status           TEXT NOT NULL CHECK (status IN ('connected', 'paused', 'needs_login')),
    last_error       TEXT,
    enabled_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
) STRICT;
INSERT INTO garmin_account (user_id, di_token, di_refresh_token, di_client_id, status, last_error, enabled_at, updated_at)
SELECT 0, di_token, di_refresh_token, di_client_id, status, last_error, enabled_at, updated_at FROM garmin_account_old;
DROP TABLE garmin_account_old;
ALTER TABLE garmin_uploads ADD COLUMN user_id INTEGER NOT NULL DEFAULT 0;

-- Sample tables: user_id joins the primary key.
ALTER TABLE mon_hr RENAME TO mon_hr_old;
CREATE TABLE mon_hr (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts  INTEGER,
    bpm REAL NOT NULL,
    src INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO mon_hr (user_id, ts, bpm, src) SELECT 0, ts, bpm, src FROM mon_hr_old;
DROP TABLE mon_hr_old;

ALTER TABLE mon_activity RENAME TO mon_activity_old;
CREATE TABLE mon_activity (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts              INTEGER NOT NULL,
    activity_type   INTEGER NOT NULL,
    steps           REAL,
    distance_m      REAL,
    active_calories REAL,
    active_time_s   REAL,
    duration_min    REAL,
    intensity       REAL,
    src             INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts, activity_type)
) STRICT, WITHOUT ROWID;
INSERT INTO mon_activity (user_id, ts, activity_type, steps, distance_m, active_calories, active_time_s, duration_min, intensity, src) SELECT 0, ts, activity_type, steps, distance_m, active_calories, active_time_s, duration_min, intensity, src FROM mon_activity_old;
DROP TABLE mon_activity_old;

ALTER TABLE mon_extra RENAME TO mon_extra_old;
CREATE TABLE mon_extra (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts           INTEGER,
    ascent_total_m     REAL,
    descent_total_m    REAL,
    moderate_total_min REAL,
    vigorous_total_min REAL,
    src          INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO mon_extra (user_id, ts, ascent_total_m, descent_total_m, moderate_total_min, vigorous_total_min, src) SELECT 0, ts, ascent_total_m, descent_total_m, moderate_total_min, vigorous_total_min, src FROM mon_extra_old;
DROP TABLE mon_extra_old;

ALTER TABLE stress RENAME TO stress_old;
CREATE TABLE stress (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts                  INTEGER,
    stress              REAL,
    stress_raw          REAL,
    body_battery        REAL,
    body_battery_status INTEGER,
    src                 INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO stress (user_id, ts, stress, stress_raw, body_battery, body_battery_status, src) SELECT 0, ts, stress, stress_raw, body_battery, body_battery_status, src FROM stress_old;
DROP TABLE stress_old;

ALTER TABLE respiration RENAME TO respiration_old;
CREATE TABLE respiration (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts       INTEGER,
    brpm     REAL,
    brpm_raw REAL,
    src      INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO respiration (user_id, ts, brpm, brpm_raw, src) SELECT 0, ts, brpm, brpm_raw, src FROM respiration_old;
DROP TABLE respiration_old;

ALTER TABLE spo2 RENAME TO spo2_old;
CREATE TABLE spo2 (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts         INTEGER,
    percent    REAL,
    confidence REAL,
    mode       INTEGER,
    src        INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO spo2 (user_id, ts, percent, confidence, mode, src) SELECT 0, ts, percent, confidence, mode, src FROM spo2_old;
DROP TABLE spo2_old;

ALTER TABLE resting_hr RENAME TO resting_hr_old;
CREATE TABLE resting_hr (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts              INTEGER,
    rhr             REAL,
    current_day_rhr REAL,
    src             INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO resting_hr (user_id, ts, rhr, current_day_rhr, src) SELECT 0, ts, rhr, current_day_rhr, src FROM resting_hr_old;
DROP TABLE resting_hr_old;

ALTER TABLE sleep_levels RENAME TO sleep_levels_old;
CREATE TABLE sleep_levels (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts         INTEGER,
    level      INTEGER NOT NULL,
    level_name TEXT,
    src        INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO sleep_levels (user_id, ts, level, level_name, src) SELECT 0, ts, level, level_name, src FROM sleep_levels_old;
DROP TABLE sleep_levels_old;

ALTER TABLE hrv_values RENAME TO hrv_values_old;
CREATE TABLE hrv_values (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts       INTEGER,
    rmssd_ms REAL NOT NULL,
    src      INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO hrv_values (user_id, ts, rmssd_ms, src) SELECT 0, ts, rmssd_ms, src FROM hrv_values_old;
DROP TABLE hrv_values_old;

ALTER TABLE hrv_summary RENAME TO hrv_summary_old;
CREATE TABLE hrv_summary (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts                      INTEGER,
    weekly_avg              REAL,
    last_night_avg          REAL,
    last_night_5min_high    REAL,
    baseline_low_upper      REAL,
    baseline_balanced_lower REAL,
    baseline_balanced_upper REAL,
    status                  INTEGER,
    src                     INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO hrv_summary (user_id, ts, weekly_avg, last_night_avg, last_night_5min_high, baseline_low_upper, baseline_balanced_lower, baseline_balanced_upper, status, src) SELECT 0, ts, weekly_avg, last_night_avg, last_night_5min_high, baseline_low_upper, baseline_balanced_lower, baseline_balanced_upper, status, src FROM hrv_summary_old;
DROP TABLE hrv_summary_old;

ALTER TABLE training_load RENAME TO training_load_old;
CREATE TABLE training_load (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts           INTEGER,
    acute        REAL,
    chronic      REAL,
    acwr         REAL,
    acwr_percent REAL,
    status       INTEGER,
    src          INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO training_load (user_id, ts, acute, chronic, acwr, acwr_percent, status, src) SELECT 0, ts, acute, chronic, acwr, acwr_percent, status, src FROM training_load_old;
DROP TABLE training_load_old;

ALTER TABLE training_status RENAME TO training_status_old;
CREATE TABLE training_status (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts              INTEGER,
    status          INTEGER,
    weekly_load_sum REAL,
    load_min        REAL,
    load_max        REAL,
    fitness_trend   INTEGER,
    load_trend      INTEGER,
    src             INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO training_status (user_id, ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend, src) SELECT 0, ts, status, weekly_load_sum, load_min, load_max, fitness_trend, load_trend, src FROM training_status_old;
DROP TABLE training_status_old;

ALTER TABLE training_balance RENAME TO training_balance_old;
CREATE TABLE training_balance (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts                      INTEGER,
    aerobic_low             REAL,
    aerobic_high            REAL,
    anaerobic               REAL,
    aerobic_low_target_min  REAL,
    aerobic_low_target_max  REAL,
    aerobic_high_target_min REAL,
    aerobic_high_target_max REAL,
    anaerobic_target_min    REAL,
    anaerobic_target_max    REAL,
    src                     INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO training_balance (user_id, ts, aerobic_low, aerobic_high, anaerobic, aerobic_low_target_min, aerobic_low_target_max, aerobic_high_target_min, aerobic_high_target_max, anaerobic_target_min, anaerobic_target_max, src) SELECT 0, ts, aerobic_low, aerobic_high, anaerobic, aerobic_low_target_min, aerobic_low_target_max, aerobic_high_target_min, aerobic_high_target_max, anaerobic_target_min, anaerobic_target_max, src FROM training_balance_old;
DROP TABLE training_balance_old;

ALTER TABLE recovery_time RENAME TO recovery_time_old;
CREATE TABLE recovery_time (
    user_id INTEGER NOT NULL DEFAULT 0,
    start_ts INTEGER,
    minutes  REAL NOT NULL,
    src      INTEGER NOT NULL,
    PRIMARY KEY (user_id, start_ts)
) STRICT, WITHOUT ROWID;
INSERT INTO recovery_time (user_id, start_ts, minutes, src) SELECT 0, start_ts, minutes, src FROM recovery_time_old;
DROP TABLE recovery_time_old;

ALTER TABLE vo2max RENAME TO vo2max_old;
CREATE TABLE vo2max (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts           INTEGER,
    maximal_met  REAL NOT NULL,,
    sport        INTEGER,
    src          INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO vo2max (user_id, ts, maximal_met, sport, src) SELECT 0, ts, maximal_met, sport, src FROM vo2max_old;
DROP TABLE vo2max_old;

ALTER TABLE battery RENAME TO battery_old;
CREATE TABLE battery (
    user_id INTEGER NOT NULL DEFAULT 0,
    ts      INTEGER,
    percent REAL NOT NULL,
    src     INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
INSERT INTO battery (user_id, ts, percent, src) SELECT 0, ts, percent, src FROM battery_old;
DROP TABLE battery_old;

-- Best-ranked file per activity, ranked among the files of one owner (0 = unassigned).
DROP VIEW activities_current;
CREATE VIEW activities_current AS
SELECT a.*, COALESCE(fo.user_id, 0) AS user_id
FROM activities a
JOIN sources s ON s.sha256 = a.sha256
LEFT JOIN file_owners fo ON fo.sha256 = a.sha256
WHERE NOT EXISTS (
    SELECT 1
    FROM activities b
    JOIN sources t ON t.sha256 = b.sha256
    LEFT JOIN file_owners fb ON fb.sha256 = b.sha256
    WHERE COALESCE(b.activity_key, b.sha256) = COALESCE(a.activity_key, a.sha256)
      AND COALESCE(fb.user_id, 0) = COALESCE(fo.user_id, 0)
      AND (t.rank_key > s.rank_key OR (t.rank_key = s.rank_key AND t.sha256 < s.sha256))
);

DROP VIEW sleep_sessions_current;
CREATE VIEW sleep_sessions_current AS
SELECT sha256, start_ts, end_ts, overall_score, quality_score, duration_score,
       recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress, user_id
FROM (
    SELECT s.*,
           COALESCE(fo.user_id, 0) AS user_id,
           ROW_NUMBER() OVER (
               PARTITION BY COALESCE(fo.user_id, 0),
                            COALESCE(CAST(s.start_ts AS TEXT), s.sha256 || ':' || s.idx)
               ORDER BY o.rank_key DESC, o.sha256 ASC
           ) AS rn
    FROM sleep_sessions s
    JOIN sources o ON o.sha256 = s.sha256
    LEFT JOIN file_owners fo ON fo.sha256 = s.sha256
)
WHERE rn = 1;
