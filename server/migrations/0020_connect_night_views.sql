-- Connect nights fill what the watch did not deliver (2026-11-28, polish L).
--
-- A `sleep_detail` Connect file brings a whole night's stage rows, HRV values and HRV summary.
-- Mixed with the watch's own rows of the same night they would interleave (different instants,
-- so the rank rule of the sample tables cannot choose between them). The rule: when the watch
-- (any FIT source) has at least one row of the table inside the night's window, the night's
-- Connect rows of that table are not shown at all.
--
-- It is a read-time rule, in these views, so it holds whatever order the files were imported in
-- and after any `reparse` (a watch file arriving later hides the Connect rows; nothing has to be
-- deleted or re-derived). A Connect row's night is the `sleep_sessions` row of its own source
-- (a `sleep_detail` file always writes one). Rows of every other source, and Connect rows that
-- have no night of their own (the daily HRV files), are shown as before.
--
-- Stage rows end their period, so their window is (start, end]; the HRV windows are [start, end].

CREATE VIEW sleep_levels_current AS
SELECT t.* FROM sleep_levels t
WHERE NOT EXISTS (
    SELECT 1
    FROM sources o
    JOIN sleep_sessions ss ON ss.sha256 = o.sha256
    WHERE o.src_id = t.src AND o.origin = 'connect'
      AND ss.start_ts IS NOT NULL AND ss.end_ts IS NOT NULL
      AND EXISTS (
          SELECT 1 FROM sleep_levels w JOIN sources ws ON ws.src_id = w.src AND ws.origin = 'fit'
          WHERE w.user_id = t.user_id AND w.ts > ss.start_ts AND w.ts <= ss.end_ts
      )
);

CREATE VIEW hrv_values_current AS
SELECT t.* FROM hrv_values t
WHERE NOT EXISTS (
    SELECT 1
    FROM sources o
    JOIN sleep_sessions ss ON ss.sha256 = o.sha256
    WHERE o.src_id = t.src AND o.origin = 'connect'
      AND ss.start_ts IS NOT NULL AND ss.end_ts IS NOT NULL
      AND EXISTS (
          SELECT 1 FROM hrv_values w JOIN sources ws ON ws.src_id = w.src AND ws.origin = 'fit'
          WHERE w.user_id = t.user_id AND w.ts >= ss.start_ts AND w.ts <= ss.end_ts
      )
);

CREATE VIEW hrv_summary_current AS
SELECT t.* FROM hrv_summary t
WHERE NOT EXISTS (
    SELECT 1
    FROM sources o
    JOIN sleep_sessions ss ON ss.sha256 = o.sha256
    WHERE o.src_id = t.src AND o.origin = 'connect'
      AND ss.start_ts IS NOT NULL AND ss.end_ts IS NOT NULL
      AND EXISTS (
          SELECT 1 FROM hrv_summary w JOIN sources ws ON ws.src_id = w.src AND ws.origin = 'fit'
          WHERE w.user_id = t.user_id AND w.ts >= ss.start_ts AND w.ts <= ss.end_ts
      )
);
