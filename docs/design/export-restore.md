# Export and restore (design, 2026-11-24)

The owner's ask: a person can take all their data out in an open format and import it into
Nimrag again, on the same server or another one. The download is then also the person's own
backup.

## What exists

- **Download my data** (`GET /api/export`, `server/src/export.rs`): a zip with `README.txt`,
  `originals/<stored name>` (every file the user owns, byte for byte) and `csv/*.csv` (one per kind
  of data, including the person's own entries: `settings`, `workouts`, `body_measurements`,
  `strength_set_edits`). Open, but nothing reads it back.
- **Server backups** (`server/src/backup.rs`, `--backup-dir`): the admin's whole-server snapshot.
  Unchanged by this design.
- **Import from Garmin** (`POST /api/import/garmin`, `server/src/import.rs`): stores every FIT it
  finds with source `import`. Given a Nimrag export it would store the FIT originals under the
  wrong source and drop the Garmin Connect files and the person's own entries.

## The archive gets an exact part

Everything is derived from the originals, so a restore needs the originals with their exact
ledger `Meta`, plus the four things a person enters by hand. The CSV files stay as they are (for
spreadsheets). New, next to them, in JSON (open, exact, no time-zone rendering):

```text
nimrag/manifest.json          {"format":"nimrag-export","version":1,"created_at":"<rfc3339>",
                               "app_version":"0.8.5","schema_version":16,"timezone":"Europe/Copenhagen",
                               "counts":{"files":..,"workouts":..,"body_measurements":..,
                               "strength_set_edits":..,"settings":..}}
nimrag/files.json             [{"name":"originals/<stored name>" | null,"sha256","size","source",
                               "data_type","sub_type","file_index","file_number","watch_timestamp",
                               "received_at"}]   (name null: the store lacked the file)
nimrag/settings.json          {"<key>":"<value as stored>", ...}
nimrag/workouts.json          [{"name","sport","definition":<the JSON value>,"created_at","updated_at"}]
nimrag/body_measurements.json [{"measured_at","weight_kg","body_fat_pct","waist_cm","created_at"}]
nimrag/strength_set_edits.json [{"activity_ident","set_idx","fields":<the JSON value>,"updated_at"}]
```

Times are Unix seconds (UTC). Written as the export streams, after the originals, with the same
rows the CSV files use (same `WHERE user_id = ?`). Nothing secret is in it: no password, token,
TOTP secret, phone, session or Garmin account. The README gains a paragraph on `nimrag/` and on
restoring.

## Restore

`POST /api/import/nimrag?name=<file name>`: body is the zip, streamed to the store's incoming dir
like the Garmin import (same size limit `max_import`, same gate `import_gate`, same detached task,
same session-cookie + CSRF rule, an admin gets 403). Profile → Account gets a row **Restore from
a Nimrag export** ("A zip you downloaded from Nimrag") opening `/profile/restore` with the same
upload UI as the Garmin import page and a result summary.

Steps, for the signed-in user only (the archive's own user does not matter):

1. Open the zip; it must hold `nimrag/manifest.json` with `format == "nimrag-export"` and a
   `version` this server knows (1). Otherwise 422 "This is not a Nimrag export". A newer version
   → 422 "made by a newer Nimrag; update this server first".
2. **Originals.** For each `files.json` entry with a name: names must be enclosed under
   `originals/` (no `..`, no absolute path); the entry is copied to incoming (capped at
   `Limits::max_fit_bytes`, hashed while copied); a hash other than `sha256` → counted as refused,
   not stored. Then `ingest::ingest(state, blob, None, Some(user), meta_from_entry, None)`.
   Ingest is idempotent: a file the server has counts as "already here" (and is claimed when it
   is unowned, as ingest does). Entries with `name: null` count as "missing in the export".
3. **Own entries**, in one transaction after the originals (so strength edits find their
   activities):
   - settings: keys `timezone`, `step_goal`, `intensity_goal_min`, `home_cards` only, each
     validated the way `PATCH /api/settings` validates it; invalid → skipped and counted. They
     replace the current values (a restore puts back what the export had).
   - workouts: added unless the user has one with the same name, sport and definition. Added ones
     get `send_state = 'idle'` (a restore never sends to the watch), revision 1, the archive's
     created/updated times.
   - body measurements: added unless one with the same `measured_at` and the same three values
     exists.
   - strength set edits: upserted when the archive's `updated_at` is newer than the current row
     (later wins, like offline edits).
4. Answer `{files: {added, already_here, missing_in_export, refused}, settings_applied,
   settings_skipped, workouts: {added, already_here}, body_measurements: {added, already_here},
   strength_set_edits: {applied, older}, errors: [..]}` (TS type generated with ts-rs).

`POST /api/import/garmin` given a zip with `nimrag/manifest.json` at the top answers 422 "This is
a Nimrag export: use Restore from a Nimrag export" instead of importing it half.

Restoring twice changes nothing the second time. Restoring into a user with data merges: nothing
of theirs is deleted.

## Not in this step

- The phone's own copy has no export or restore (both need the server, like the Garmin import).
- Exports made before this change (no `nimrag/` folder) are refused with "made by an older Nimrag:
  download a new export".
- Deletions are not carried: a restore only adds and updates.

## Tests that must exist

- Round trip (server test): user A with a watch FIT, a Connect envelope, an `import` FIT, settings,
  two workouts, two body entries and a strength edit → export → restore into user B on a fresh
  server: same files with the same sources, same counts in activities, daily totals and sleep,
  equal own entries (send_state idle). Restore again → everything "already here".
- Restore into user A itself → nothing added.
- A hash mismatch is refused and not stored; a `../` name is refused; a zip without a manifest → 422;
  a newer version → 422; admin 403; no CSRF → refused.
- The Garmin import refuses a Nimrag export with the message above.
- The export's `nimrag/*.json` holds only that user's rows (another user's workout is absent).
- Web: the restore page uploads and shows the summary; the profile row links to it.

## Changes while building

- **Restored files are not forwarded.** `ingest::ingest` queues a new watch activity for Garmin
  Connect when the user has Forward on. A restore puts old files back and must not upload them
  again, so `ingest` is now a thin wrapper of `ingest::ingest_with(.., forward)` and the restore
  calls it with `forward = false`. Everything else is `ingest` as specified.
- **Older exports.** A zip with no manifest is "not a Nimrag export" (422), except one that has
  `README.txt` and `csv/files.csv` (an export from before this change): that gets "This export was
  made by an older Nimrag: download a new export", as the "Not in this step" list says.
- **Export fix.** The export named a Garmin Connect JSON original without `.json` (it read only 12
  bytes to tell it from FIT, the envelope magic is longer). The names now follow the README
  (`originals/<sha>.json`). `files.json` always carries the exact name used.
- **Manifest.** `app_version` is the server's `CARGO_PKG_VERSION`, `schema_version` the highest
  applied migration. `created_at` is the time of the export, so the command-line export and the
  download differ in that one field.
- **Limits and errors.** Each JSON part is read up to 64 MiB. A listed file absent from the zip, or
  with a source that is not a plain word, counts as `refused`. A workout over the 200-per-user
  limit is left out with an error text. A missing or unreadable JSON part is 422 "This export is
  damaged: <part>". The 422 texts are the full sentences in `server/src/restore.rs`.
- **Upload queue.** The web restore page shares the Garmin import's upload queue
  (`useUploadQueue` in `web/src/pages/profile/garminImport.ts`).
