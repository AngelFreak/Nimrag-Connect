-- Forward can send every watch file, not only activities (Stage 2g, 2026-11-22).
--
-- * `garmin_account.send_all`: 0 = activities only (what Forward did until now), 1 = every FIT file
--   the watch produces (monitoring, sleep, metrics, ...), so Garmin Connect stays complete. A new
--   account starts at 0; the owner changes it on the Garmin page.
-- * `garmin_uploads.order_ts`: when the file is from, in unix milliseconds. The worker sends the
--   oldest first (a backfill queues many files at the same instant). NULL = queued live: its
--   `created_at` is the order.
ALTER TABLE garmin_account ADD COLUMN send_all INTEGER NOT NULL DEFAULT 0 CHECK (send_all IN (0, 1));
ALTER TABLE garmin_uploads ADD COLUMN order_ts INTEGER;
