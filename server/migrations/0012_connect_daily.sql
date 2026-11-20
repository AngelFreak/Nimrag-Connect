-- Garmin Connect daily summaries (2026-11-20).
--
-- `import-connect` also reads `daily_stats/YYYY-MM.json` (one summary per local day: stress, body
-- battery, respiration, intensity minutes, heart rate range, floors, distance). The history
-- before the phone started syncing has no watch data in these metrics, so the pages fall back
-- to this table for a day (and a metric) the watch has no data for; a watch row always wins
-- (see server/README.md "Garmin Connect history").
--
-- A sample table like `daily_totals`: natural key (user, start of the local day) + `src`, a
-- rank-based upsert (a Connect source ranks below every FIT source), cleared and rebuilt by
-- reparse. Every value column is NULL when Connect gave none, never 0.
CREATE TABLE connect_daily (
    user_id      INTEGER NOT NULL DEFAULT 0,
    ts           INTEGER NOT NULL,
    steps        REAL,
    distance_m   REAL,
    floors_up    REAL,
    moderate_min REAL,
    vigorous_min REAL,
    hr_min       REAL,
    hr_max       REAL,
    stress_avg   REAL,
    bb_charged   REAL,
    bb_drained   REAL,
    bb_high      REAL,
    bb_low       REAL,
    resp_avg     REAL,
    resp_high    REAL,
    resp_low     REAL,
    src          INTEGER NOT NULL,
    PRIMARY KEY (user_id, ts)
) STRICT, WITHOUT ROWID;
