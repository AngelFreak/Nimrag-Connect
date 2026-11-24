# Scheduled backups set in the web app (design, 2026-11-24)

The owner's ask: scheduled backups as files on the server's file system, at a frequency and in a
folder the admin chooses, in the web app instead of start-up options only.

## What exists

`server/src/backup.rs`: `run_once` writes `nimrag-backup-<UTC>/` (database snapshot via
`VACUUM INTO`, the file store with hard links to the previous backup, `backup.json`), prunes to
`keep`, refuses a folder inside the data dir, locks the folder. `spawn_scheduler` runs it every
`every` from a fixed `Config` given at start-up by `--backup-dir/--backup-every/--backup-keep`
(or `NIMRAG_BACKUP_*`). `nimrag-server restore --from <backup> --data-dir <new>` restores one.
None of that changes; only where the config comes from and that it can change while running.

## Settings

Stored in the global `settings` table (no migration needed: key/value):
`backup_enabled` (`0`/`1`), `backup_dir` (absolute path), `backup_every_s`, `backup_keep`.

- When the admin has saved backup settings in the web app, they are used.
- Otherwise the start-up options are used, as today (and when there are none: off).
- The admin page says which ("Set at server start" / "Set here") and has **Use start-up settings**
  (deletes the four keys) when saved settings exist and start-up options were given.

Validation on save (422 with a plain reason):
- `dir`: absolute; exists and is a directory ("This folder does not exist on the server. With
  Docker, mount a folder into the container first, for example /backups."); writable (create and
  remove a probe file `.nimrag-write-test`); not inside the data dir and the data dir not inside it
  (`refuse_inside`); at most 1024 bytes.
- `every_s`: 3600 to 30 days. The page offers Every 6 hours, Every 12 hours, Daily, Every 2 days,
  Weekly, and Custom (whole hours, 1 to 720).
- `keep`: 1 to 365.
- `enabled = false` keeps the other values (off, not forgotten).

## Running

The scheduler takes its config from a `tokio::sync::watch` channel (`AppState::backup_config`,
`Option<Config>`); saving a new config sends on it and the loop recomputes the wait at once (a
shorter interval or a new folder can make a backup due now: same `due_in` rule, the newest backup
in the new folder counts). Off = the loop waits for the next change.

Status kept in memory in `AppState` (a `Mutex`), not persisted: `running` (bool), `last_ok`
(name, finished at, duration, bytes copied), `last_error` (text without the data dir path, at),
`next_at` (unix seconds, null when off). One backup at a time: a run already going makes **Back up
now** answer 409.

## API (admin only, like `/api/admin/users`: session of an admin, CSRF header on writes)

- `GET /api/admin/backups` →
  `{settings: {enabled, dir, every_s, keep} | null, source: "saved" | "startup" | "none",
  startup: {dir, every_s, keep} | null, status: {running, last_ok, last_error, next_at},
  backups: [{name, at, files, files_bytes, db_bytes}] }` — `backups` is `backup::list` of the
  active folder, newest first, sizes from each `backup.json` (null when unreadable); empty when the
  folder cannot be read (with `status.last_error` explaining).
- `PUT /api/admin/backups` `{enabled, dir, every_s, keep}` → validates, saves, applies; answers
  like GET.
- `DELETE /api/admin/backups` → back to start-up settings; answers like GET.
- `POST /api/admin/backups/run` → starts one backup now into the active folder (with pruning),
  202; 409 when one is running; 422 when backups have no folder.

Types generated with ts-rs into `web/src/api/generated/`.

## Web

Admin page (`web/src/pages/AdminPage.tsx`, cards in `web/src/pages/admin/`): a **Backups** card
under the users:

- Status line: "Last backup today 03:00 · 213 MB copied · next in 21 h", or "Off", or the last
  error in the danger tone with when it happened, or "Backing up…" while running (poll GET every
  3 s while running).
- Form: On/off switch, Folder (text field, monospace), How often (select with the presets above;
  Custom shows an hours field), Keep (number, "newest backups"). Save; errors under the field.
  The source line and "Use start-up settings".
- **Back up now** button.
- List of the backups in the folder: time (in the admin's time zone), size (db + files), name.
- One line on restoring: "To restore, stop the server and run `nimrag-server restore --from
  <folder>/<backup> --data-dir <new folder>` (see the runbook)."

## Not in this step

- No folder browser on the server (a typed path, validated).
- No restore from the web app (it replaces the whole server; it stays a command).
- No upload of backups elsewhere (point the folder at a NAS mount or a synced folder).

## Tests that must exist

- Server: saved settings beat start-up options; DELETE goes back to them; each validation case
  (relative path, missing folder, not writable, inside the data dir, every/keep bounds); a
  non-admin gets 403 and no CSRF is refused; PUT with a short interval and an empty folder makes
  a backup happen promptly (the scheduler reacts to the change, use a short test interval hook if
  needed); run-now 202 then 409 while running; the list reports the backups with sizes; off stops
  scheduling.
- Web: the card shows status, saves, shows a validation error, Custom hours, Back up now,
  the list.

## Changes while building

- Errors of a refused PUT (and of run-now without a folder) are 422 with `{error, field}` where
  `field` is `dir`, `every_s` or `keep`, so the page can show the text under the right field.
- `enabled = false` skips the folder checks (existence, writable, overlap) so backups can be switched
  off when the folder has gone; the folder must then still be an absolute path or empty (empty only
  when off). Bounds on `every_s` and `keep` always apply.
- The overlap check refuses a folder that contains the data dir as well as one inside it
  (`backup::check_folder`); `run_once` keeps its older "inside only" rule, so `--backup-dir` behaves as
  before.
- `Back up now` uses the saved folder and keep count also while backups are off, else the start-up
  ones. It answers 202 with the same body as GET (running = true).
- Status times (`last_ok.at`, `last_error.at`, `next_at`) and list times are unix seconds; the web app
  formats them in the admin's time zone. After a restart `last_ok` is empty, and the page falls back to
  the newest backup in the list.
- `status.last_error` is cleared by the next success. When the folder cannot be listed, GET fills it
  with "The backup folder cannot be read." (nothing is persisted).
- Saved values that are incomplete or unparseable are ignored (logged), so start-up options apply.
- `backup::spawn_scheduler(pool, data_dir, config)` still exists (fixed config); `serve` uses
  `spawn_scheduler_with(.., Arc<Control>)`.
- Run now while one runs is tested by holding the backup gate (`Control::try_begin`), since a backup of
  the test database ends in milliseconds.
