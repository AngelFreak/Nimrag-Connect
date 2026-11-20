-- Garmin Connect history import (2026-11-20).
--
-- The owner's Garmin Connect history (pulled as JSON) is stored as content-addressed blobs of
-- their own kind and derived into the same tables as the watch's FIT files, as sources of lower
-- rank (`sources.origin = 'connect'`, see server/README.md "Garmin Connect history").
--
-- * `files.source` accepts 'connect' (the CHECK is rebuilt: SQLite cannot alter it, and `files` is
--   the parent of about twenty tables, so the rows go through a temp table with the foreign keys
--   deferred to COMMIT; nothing is lost, see tests/migration.rs).
-- * `sources.origin` tells FIT files ('fit', every existing row) from Connect JSON ('connect').
-- * `sleep_sessions` gets the stage totals a Connect night carries (a watch night has stage rows in
--   `sleep_levels` instead): asleep, deep, light, REM and awake seconds.
-- * `daily_totals` holds Garmin's whole-day steps and calories; the Today and Movement pages use
--   them for a day without minute data.
-- * `activities_current` and `sleep_sessions_current` hide a Connect activity or night when a FIT
--   file of the same user has the same one.

PRAGMA defer_foreign_keys = ON;
CREATE TEMP TABLE files_bak AS SELECT * FROM files;
DROP TABLE files;
CREATE TABLE files (
    sha256          TEXT PRIMARY KEY CHECK (length(sha256) = 64),
    size            INTEGER NOT NULL,
    received_at     TEXT NOT NULL,
    source          TEXT NOT NULL CHECK (source IN ('watch', 'usb', 'import', 'connect')),
    data_type       INTEGER,
    sub_type        INTEGER,
    file_index      INTEGER,
    file_number     INTEGER,
    watch_timestamp INTEGER
);
INSERT INTO files SELECT * FROM files_bak;
DROP TABLE files_bak;

ALTER TABLE sources ADD COLUMN origin TEXT NOT NULL DEFAULT 'fit' CHECK (origin IN ('fit', 'connect'));

ALTER TABLE sleep_sessions ADD COLUMN asleep_s REAL;
ALTER TABLE sleep_sessions ADD COLUMN deep_s REAL;
ALTER TABLE sleep_sessions ADD COLUMN light_s REAL;
ALTER TABLE sleep_sessions ADD COLUMN rem_s REAL;
ALTER TABLE sleep_sessions ADD COLUMN awake_s REAL;

-- A sample table (natural key + src, rank-based upsert, cleared and rebuilt by reparse).
-- ts is the start of the local day the totals are for.
CREATE TABLE daily_totals (
    user_id     INTEGER NOT NULL DEFAULT 0,
    ts          INTEGER NOT NULL,
    steps       REAL,
    total_kcal  REAL,
    active_kcal REAL,
    resting_kcal REAL,
    src         INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;

-- Best-ranked file per activity (one owner at a time, 0 = unassigned), minus a Connect activity
-- that a FIT activity of the same owner starting within 120 s of it has superseded.
DROP VIEW activities_current;
CREATE VIEW activities_current AS
SELECT a.*, COALESCE(fo.user_id, 0) AS user_id, s.origin AS origin
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
)
AND NOT (
    s.origin = 'connect'
    AND a.start_ts IS NOT NULL
    AND EXISTS (
        SELECT 1
        FROM activities f
        JOIN sources fs ON fs.sha256 = f.sha256 AND fs.origin = 'fit'
        LEFT JOIN file_owners ff ON ff.sha256 = f.sha256
        WHERE COALESCE(ff.user_id, 0) = COALESCE(fo.user_id, 0)
          AND f.start_ts IS NOT NULL
          AND f.start_ts BETWEEN a.start_ts - 120 AND a.start_ts + 120
    )
);

-- Best-ranked session per (owner, start); a Connect night is hidden by a FIT night of the same
-- owner that overlaps it in time.
DROP VIEW sleep_sessions_current;
CREATE VIEW sleep_sessions_current AS
SELECT sha256, start_ts, end_ts, overall_score, quality_score, duration_score,
       recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress,
       asleep_s, deep_s, light_s, rem_s, awake_s, user_id
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
    WHERE NOT (
        o.origin = 'connect'
        AND s.start_ts IS NOT NULL AND s.end_ts IS NOT NULL
        AND EXISTS (
            SELECT 1
            FROM sleep_sessions w
            JOIN sources ws ON ws.sha256 = w.sha256 AND ws.origin = 'fit'
            LEFT JOIN file_owners wf ON wf.sha256 = w.sha256
            WHERE COALESCE(wf.user_id, 0) = COALESCE(fo.user_id, 0)
              AND w.start_ts IS NOT NULL AND w.end_ts IS NOT NULL
              AND w.start_ts < s.end_ts AND w.end_ts > s.start_ts
        )
    )
)
WHERE rn = 1;
