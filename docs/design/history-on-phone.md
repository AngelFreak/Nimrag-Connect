# The server's history on the phone (design, 2026-11-24)

The owner's ask: the phone's copy should show the same as the web, also for the history that only
the server has (Garmin Connect imports, a Garmin data export, files another phone uploaded).

## Why this is small

Everything the server shows is derived from stored files: watch FIT files, FIT files from an
export (`source = 'import'`), and Garmin Connect JSON in an envelope (`source = 'connect'`, see
`server/src/connect/mod.rs`). Derivation needs nothing but the bytes and the ledger `Meta`, and
the phone's copy (`server/src/embedded.rs`) runs the same ingest and derive code. So the phone
copies the **files** it does not have and ingests them like its own; the derived rows then match
the server's (Connect ranks below FIT whatever the order, `sources.origin`).

Size for the owner today: about 800 files the phone lacks, about 75 MB, once. Later only what is
new on the server.

## The person's choice: "Data retention"

The copy is opt-in. The native settings screen has a "Data retention" section with **Server
history on this phone**: Off (default: nothing is copied), Last 3 months, Last 12 months,
Everything, and one line of explanation ("Copies history that only the server has (Garmin
imports, other phones) so it shows offline too. Your watch's own data is always kept.").

- The window is by **data date**, not upload date: a file counts when its last data timestamp
  (`sources.last_ts`) is at or after now minus the window, or when that is unknown.
- Off runs no history step at all. Everything passes no window.
- Widening the choice starts the history step right away.
- Shrinking it (or Off) removes the copied server files outside the new window from the phone and
  rebuilds the derived data. Files the phone got from its own watch (`ingest_file`) are never
  removed: copied files are marked (`history_copies`), the rest are not.

## Server

Phone-token routes (like the other `/api/device/*`), the token's user only (`file_owners`); a
token without a user gets 403.

- `GET /api/device/files?cursor=&limit=&since=` (limit default 500, max 1000): the user's files,
  **newest first** (`received_at DESC, sha256 ASC`, keyset cursor, opaque string), as
  `{files: [{sha256, size, source, data_type, sub_type, file_index, file_number,
  watch_timestamp, received_at, last_ts}], next_cursor: string | null}`. `last_ts` is
  `sources.last_ts` (the file's last data timestamp, unix seconds; null when unknown). `since`
  (unix seconds) keeps files with `last_ts >= since` and those with a null `last_ts`.
- `GET /api/device/files/{sha256}/content`: the bytes (`application/octet-stream`), read with
  `store::read_verified`; 404 when it is not this user's file (same rule as `file_info`).

## Phone

FFI on `LocalServer` (`ffi/src/local_server.rs`):

- `missing_files(sha256s: Vec<String>) -> Vec<String>`: those of the list the copy does not have.
- `ingest_history_file(bytes, meta: HistoryFileMeta) -> bool`: like `ingest_file`, but with the
  server's `source` string and optional fields (`HistoryFileMeta {sha256, source, data_type?,
  sub_type?, file_index?, file_number?, watch_timestamp?, last_ts?}`). Refuses (an error) bytes
  whose sha256 is not `meta.sha256`, so a broken download is never stored. The file is marked as
  a copy (`history_copies`, with `last_ts`) unless the phone already had it, and a file the
  phone's own watch delivers later loses the mark, so it is never pruned.
- `prune_history(keep_since: Option<i64>) -> u32`: removes the marked copies with a known
  `last_ts` before `keep_since` (all marked copies when `None`, which is what Off means), their
  bytes and derived rows, rebuilds the derived data of the rest, and returns how many files went.

The engine (`UploadWorker`, after file uploads, edits and the state pull, same network rule and
token handling) runs a **history step** when the setting is not Off:

0. A changed setting is noticed at the start of a pass: narrower or Off → `prune_history`; wider
   → look again at once. The window is passed as `since` (now minus 3 or 12 months).
1. Page through `GET /api/device/files`; collect what `missing_files` says is missing.
2. Download and ingest those, newest first, at most 25 files or 10 MB per pass, then the pass
   ends with a short wait (0) so the next pass continues; uploads and edits keep their turn.
3. When the list has been walked with nothing missing, remember `historyCheckedAt` and look again
   only after new uploads reached the server, on app start, and at most every 6 hours.
4. Failures: IO/5xx → the history step backs off (its own backoff, never the uploads'); 401/403
   → same as uploads; 404 on a content download (file gone meanwhile) → skip it.
5. Progress: one log line per pass ("Copied 25 of the server's files to the phone (310 left)")
   and a final one. The Watch page's sync history shows them; no new screen.

The engine's own file store (the upload queue) is **not** used: history files go only into the
local server's store, so they are never uploaded back.

## Not in this step

- Files removed from the user on the server (admin unassign) stay on the phone.
- Desktop browsers keep using the server only.

## Tests that must exist

- Server: list pagination and order, `since` and `last_ts`, user isolation (other user's file → not listed, content
  404), token without user 403, content bytes equal the stored blob.
- Embedded/FFI: `missing_files`; `ingest_history_file` stores and derives a Connect envelope and a
  FIT, refuses a sha mismatch, is idempotent; `prune_history` removes copies outside the window, keeps the watch's own files
  and rebuilds the derived data.
- Android (JVM tests on the pure parts): paging, the per-pass limit, newest first, backoff and
  the 6-hour rule.
