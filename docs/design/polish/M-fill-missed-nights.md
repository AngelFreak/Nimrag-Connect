# M. Fill missed nights from Garmin, every morning

## Why

The watch hands each file to whichever app on the phone connects first, and only once. When the
Garmin Connect app syncs first, Nimrag never gets that night's stages (FIT 49), HRV (68) and the
morning status file. Garmin has them. Spec L added the `sleep_detail` Connect kind and the
read-time rule "watch rows win". This spec makes the server fetch those nights by itself for users
whose Garmin account is connected (Forward), so nobody has to run `import-connect` by hand.

The stored DI login already carries the `CONNECT_READ` scope, so no new login is needed.

## 1. Data (migration 0021)

- `garmin_account.fill_nights INTEGER NOT NULL DEFAULT 1 CHECK (fill_nights IN (0, 1))`.
- New table `garmin_night_checks`:
  `user_id INTEGER NOT NULL, date TEXT NOT NULL ('YYYY-MM-DD', the local day the night ends),
  checked_at INTEGER NOT NULL (unix s), outcome TEXT NOT NULL CHECK (outcome IN ('filled',
  'no_night', 'error')), PRIMARY KEY (user_id, date)) STRICT, WITHOUT ROWID`.
  One row per user and date, overwritten by each check. Deleting the Garmin account deletes the
  user's rows (in `delete_account`). Add the table wherever per-user tables are listed (user
  delete, export/restore exclusion lists, etc.: grep how `garmin_uploads` is handled and do the
  same; it is server state, not user data, so it is NOT exported).

## 2. The morning job (`server/src/garmin/nights.rs`)

Spawned in `main.rs` next to `queue::spawn_worker`. It runs a pass 2 minutes after start, then
every 30 minutes. A pass, for every user with `user_id > 0`, `status = 'connected'`,
`fill_nights = 1`, in that user's time zone (`settings` prefs `tz`):

1. Candidate dates: the last 7 local dates ending today. Today is a candidate only from 09:00 local
   time (the watch usually syncs by then; give it the chance first).
2. A date is skipped when:
   - the watch already has the night: a `sleep_levels` row from a FIT-source file (same notion of
     "FIT source" as migration 0020's views) with `ts` in [date−1 18:00 local, date 14:00 local);
   - its check row says `filled`;
   - its check row is younger than 3 hours (`no_night` or `error`), so one date is asked at most
     every 3 hours.
3. At most 7 Garmin requests per user per pass, with a pause of at least 2 s between two requests
   (configurable for tests the way `upload_pause_ms` is).
4. Fetch: `GET {connectapi}/wellness-service/wellness/dailySleepData/{displayName}?date=YYYY-MM-DD&nonSleepBufferMinutes=60`
   with `auth::api_headers`. `displayName` comes from `GET {connectapi}/userprofile-service/socialProfile`
   (field `displayName`), asked once per process per user and kept in memory only. Never store or
   log it, the tokens or the response body.
5. Answer handling, the same way as the upload worker:
   - 401/403 → refresh the tokens once (reuse the queue's refresh under the token lock; make it
     `pub(crate)` rather than copying it) and retry once; refresh refused → `expire_login`, stop
     this user's pass.
   - 429 → record `error` for that date, stop this user's pass (honour `Retry-After` up to the
     next pass, no busy loop).
   - other non-2xx or unreadable JSON → record `error`, go on with the next date.
   - 2xx: the JSON must classify as `Kind::SleepDetail` (`import::classify`) and have a non-empty
     `sleepLevels` and a numeric `dailySleepDTO.sleepStartTimestampGMT`; otherwise record
     `no_night`.
6. Store it exactly as `import-connect` stores a `sleep_detail/YYYY-MM-DD.json` file: factor the
   per-file body of `connect::import::run` into a reusable
   `pub async fn import_one(state, origin: &str, raw: &str, owner: i64, tz: Tz) -> Result<Report>`
   and call it with origin `garmin-fetch/sleep_detail/YYYY-MM-DD.json`. `run` keeps its behaviour
   and uses the same function. Source stays `"connect"`, so `garmin::wanted` never sends it back
   to Garmin (add a test that proves no `garmin_uploads` row appears). Ingest derives as usual.
   Then record `filled`.
7. Re-check right before the fetch (step 2's watch test) so a watch file that arrived during the
   pass wins. Watch files that arrive after the fetch already win through the 0020 views.
8. One log line per pass at info with counts only (`users`, `asked`, `filled`, `no_night`,
   `errors`), never ids, names or dates of a user.

## 3. API and web

- The existing Garmin account endpoint (`server/src/garmin/api.rs`, where `send_all` is read and
  set) gains `fill_nights` (bool) on read and on write, plus `last_filled_night`
  (`'YYYY-MM-DD'` or null: the newest `filled` date). Regenerate the web API types the way the
  repo does it.
- `web/src/pages/profile/GarminPage.tsx`: a switch next to the "send all" one:
  **"Fill missed nights from Garmin"**, help line: "When the Garmin Connect app syncs your watch
  first, Nimrag misses that night's sleep stages and HRV. Nimrag then fetches the night from your
  Garmin account each morning. Your watch's own files always win." Below it, when set:
  "Last night filled: {date}". Same look and behaviour as the existing switch (web and app).
- `server/README.md`: one paragraph in the Forward / Garmin section.

## Checks

- Server tests with the local fake Garmin the existing garmin tests use (`GarminUrls::all_on`):
  a missing night is fetched once and filled (stage rows, HRV rows visible through the `_current`
  views); a night the watch has is never asked; `filled` is never asked again; `no_night` is asked
  again only after 3 h; today before 09:00 local is not asked; `fill_nights = 0` and a paused or
  `needs_login` account ask nothing; 401 → refresh → retry works; refused refresh → `needs_login`;
  429 stops the user's pass; at most 7 requests per pass; the stored blob has no account ids; no
  `garmin_uploads` row for the fetched blob; the API reads and writes `fill_nights` and
  `last_filled_night`. Use synthetic JSON only.
- Web test for the switch and the "Last night filled" line.
- `cargo fmt --check`, `cargo test -p nimrag-server`, clippy on the files you touch (main has five
  older `-D warnings` lints in history.rs, settings.rs, health_status.rs; leave them), and in
  `web/`: typecheck, lint, vitest, build.
