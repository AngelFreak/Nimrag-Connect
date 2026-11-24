-- Restless moments of a night (2026-11-24, sleep polish D).
--
-- A night imported from Garmin Connect carries a count of restless moments; a watch night does
-- not (NULL). The "current" view shows it with the other night columns.
ALTER TABLE sleep_sessions ADD COLUMN restless_moments REAL;

DROP VIEW sleep_sessions_current;
CREATE VIEW sleep_sessions_current AS
SELECT sha256, start_ts, end_ts, overall_score, quality_score, duration_score,
       recovery_score, deep_score, light_score, rem_score, awakenings_count, avg_stress,
       asleep_s, deep_s, light_s, rem_s, awake_s, user_id, restless_moments
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
