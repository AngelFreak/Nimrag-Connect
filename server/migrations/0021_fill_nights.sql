-- Fill missed nights from Garmin, every morning (2026-11-28, polish M).
--
-- * `garmin_account.fill_nights`: 1 = the server fetches a night's sleep record from the user's
--   Garmin account when the watch never delivered it (on by default), 0 = never.
-- * `garmin_night_checks`: one row per user and local date (the day the night ends), overwritten
--   by each check: when it was asked and what came of it. Server state, not user data: it is not
--   exported, and it goes with the Garmin account.
ALTER TABLE garmin_account ADD COLUMN fill_nights INTEGER NOT NULL DEFAULT 1 CHECK (fill_nights IN (0, 1));

CREATE TABLE garmin_night_checks (
    user_id    INTEGER NOT NULL,
    date       TEXT    NOT NULL,
    checked_at INTEGER NOT NULL,
    outcome    TEXT    NOT NULL CHECK (outcome IN ('filled', 'no_night', 'error')),
    PRIMARY KEY (user_id, date)
) STRICT, WITHOUT ROWID;
