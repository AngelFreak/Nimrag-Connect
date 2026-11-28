# L. Connect nights fill what the watch did not deliver

## Why

Some mornings the watch's sleep-stage, HRV and morning training-status files never reach
Nimrag (2026-11-28: only the night summary arrived), while Garmin Connect has the whole night.
Health status also says "Calibrating" for heart rate and breathing because the nights before the
watch synced through Nimrag (before 18 Nov) have no minute-level heart rate or breathing.
Garmin Connect's per-night sleep record has all of it. `import-connect` should accept that record
and fill the gaps, with the watch's own rows always winning.

## 1. New Connect kind `sleep_detail` (server/src/connect)

Input: one JSON per night (`sleep_detail/YYYY-MM-DD.json`), Connect's full sleep response for
a date. Recognise it by shape: a top-level object with `dailySleepDTO` (object) and
`sleepLevels` (array). Fields used (all optional except `dailySleepDTO.calendarDate`; skip any
entry with a missing/non-numeric value; instants are UTC epoch milliseconds unless noted):

| field | meaning |
|---|---|
| `dailySleepDTO.calendarDate` | "YYYY-MM-DD", the night's date (the day it ends) |
| `dailySleepDTO.sleepStartTimestampGMT`, `sleepEndTimestampGMT` | night start/end, ms |
| `dailySleepDTO.deepSleepSeconds`, `lightSleepSeconds`, `remSleepSeconds`, `awakeSleepSeconds`, `sleepTimeSeconds` | stage totals |
| `dailySleepDTO.sleepScores.overall.value` | score |
| `sleepLevels[]`: `{startGMT, endGMT, activityLevel}` | stage periods; `startGMT`/`endGMT` are UTC strings like `"2026-11-27T22:23:00.0"` (no zone, they are UTC); `activityLevel` 0.0 = deep, 1.0 = light, 2.0 = REM, 3.0 = awake |
| `hrvData[]`: `{value, startGMT}` | overnight HRV (rmssd ms) every 5 min; `startGMT` ms |
| `avgOvernightHrv` | the night's HRV average (ms) |
| `sleepHeartRate[]`: `{value, startGMT}` | heart rate (bpm) every 2 min; ms |
| `wellnessEpochRespirationDataDTOList[]`: `{startTimeGMT, respirationValue}` | breathing (brpm) every minute; ms; values <= 0 are gaps |

Strip account identifiers before the envelope is made (like the intraday kinds drop
`userProfilePK`): drop `dailySleepDTO.id` and `dailySleepDTO.userProfilePK` and any top-level
`userProfilePK`. Keep the rest byte for byte as the other kinds do.

Rows it writes (through the same writers/rank rules as the other Connect kinds, so a FIT row at
the same instant wins and a Connect blob never overrides watch data):

- `sleep_levels`: one row per `sleepLevels` period, `ts` = the period's END (rows end their period,
  see `levels()` in `web_api/daily.rs`), mapped to Nimrag's levels: Connect 0 -> 3 "deep",
  1 -> 2 "light", 2 -> 4 "rem", 3 -> 1 "awake".
- `hrv_values`: one row per `hrvData` entry (`ts` = startGMT seconds, `rmssd_ms` = value).
- `hrv_summary`: one row at the night's end with `last_night_avg` = `avgOvernightHrv` (other
  columns NULL).
- `mon_hr`: one row per `sleepHeartRate` entry (same shape the intraday heart-rate kind writes).
- `respiration`: one row per breathing entry with a value > 0 (same as the intraday kind).
- a night in `sleep_sessions` exactly as the existing `Sleep` kind writes one for a Connect night
  (start/end, score, stage totals). The existing rule already hides a Connect night behind a FIT
  night at the same time — keep that. When a FIT night exists, the stage rows above still show
  for it (the web reads `sleep_levels` by time).

Important: a watch night that HAS its own stage rows must not get Connect stage rows mixed in.
Rows at different instants would otherwise interleave. Rule: for `sleep_levels`, `hrv_values` and
`hrv_summary`, skip the whole set from a `sleep_detail` blob when the watch (any FIT source) already
has at least one row of that table inside the night's [start, end] window for that user. (`mon_hr`
and `respiration` follow the existing per-instant rule of the intraday kinds.) Make sure
`reparse` gives the same result whatever order files were imported in (watch files arriving later
must win: if a FIT file later brings stage rows for that night, the Connect rows of that night
must no longer show). If doing that at derive time is hard, do it at read time instead: in
`levels()`/HRV reads, prefer FIT-ranked rows when a night has any. Pick the simplest correct way
and explain it in the code comment.

Docs: add the kind to the input table in `server/README.md` (the `import-connect` section).

## 2. Health status: leave out what has never been measured

`web/src/pages/today/tiles.tsx` `HealthStatusTile` and the Health status page
(`web/src/pages/health/` — find what renders `useHealthStatus`): a metric whose state is
`no_reading` AND that has no value in any of the earlier nights of the window (`nights == 0` in the
API, check the field name in `server/src/web_api/health_status.rs` / generated types) is left out
entirely, not shown as "No reading". A metric that has history but no value tonight still shows
"No reading". If every metric is left out, the tile shows one calm line ("No overnight readings
yet.").

## Checks

- Server tests: a `sleep_detail` fixture (write your own small synthetic JSON with the shape
  above; 3–4 levels, a few HRV/HR/breathing points) imported into an empty DB gives the stage rows
  with the right levels and end instants, HRV rows, the summary, HR and breathing rows; the same
  import for a night whose watch file has stage rows adds no Connect stage rows; account ids are not
  in the stored blob; `/api/sleep` (or the night endpoint the web uses) shows the Connect stages
  for a FIT night that had none.
- Web tests for the health-status rule.
- `cargo fmt --check`, `cargo clippy -p nimrag-server --all-targets -- -D warnings`,
  `cargo test -p nimrag-server`, and in `web/`: typecheck, lint, vitest, build.
