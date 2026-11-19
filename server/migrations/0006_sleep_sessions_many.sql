-- A file can hold several sleep summaries (the watch's backup keeps a history of
-- nights), so sleep_sessions is keyed by (sha256, idx): idx is the position of the
-- session among the file's sessions, which is deterministic for a given file.
-- Existing rows become idx 0; DERIVE_VERSION 3 makes `reparse` refill the table.
DROP VIEW sleep_sessions_current;

CREATE TABLE sleep_sessions_new (
    sha256           TEXT NOT NULL REFERENCES files (sha256),
    idx              INTEGER NOT NULL,
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
    avg_stress       REAL,
    PRIMARY KEY (sha256, idx)
) STRICT, WITHOUT ROWID;

INSERT INTO sleep_sessions_new
SELECT sha256, 0, start_ts, end_ts, overall_score, quality_score, duration_score,
       recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress
FROM sleep_sessions;

DROP TABLE sleep_sessions;
ALTER TABLE sleep_sessions_new RENAME TO sleep_sessions;

-- Best-ranked file per sleep (natural key start_ts; a session without a start
-- time is its own key). Same rank as sources: ok, last_ts, msg_count, sha256.
CREATE VIEW sleep_sessions_current AS
SELECT sha256, start_ts, end_ts, overall_score, quality_score, duration_score,
       recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress
FROM (
    SELECT s.*,
           ROW_NUMBER() OVER (
               PARTITION BY COALESCE(CAST(s.start_ts AS TEXT), s.sha256 || ':' || s.idx)
               ORDER BY o.rank_key DESC, o.sha256 ASC
           ) AS rn
    FROM sleep_sessions s JOIN sources o ON o.sha256 = s.sha256
)
WHERE rn = 1;
