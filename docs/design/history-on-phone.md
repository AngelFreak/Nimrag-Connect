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

## Server

Phone-token routes (like the other `/api/device/*`), the token's user only (`file_owners`); a
token without a user gets 403.

- `GET /api/device/files?cursor=&limit=` (limit default 500, max 1000): the user's files,
  **newest first** (`received_at DESC, sha256 ASC`, keyset cursor, opaque string), as
  `{files: [{sha256, size, source, data_type, sub_type, file_index, file_number,
  watch_timestamp, received_at}], next_cursor: string | null}`.
- `GET /api/device/files/{sha256}/content`: the bytes (`application/octet-stream`), read with
  `store::read_verified`; 404 when it is not this user's file (same rule as `file_info`).

## Phone

FFI on `LocalServer` (`ffi/src/local_server.rs`):

- `missing_files(sha256s: Vec<String>) -> Vec<String>`: those of the list the copy does not have.
- `ingest_history_file(bytes, meta: HistoryFileMeta) -> bool`: like `ingest_file`, but with the
  server's `source` string and optional fields (`HistoryFileMeta {source, data_type?, sub_type?,
  file_index?, file_number?, watch_timestamp?}`). Refuses bytes whose sha256 is not the one
  expected (pass it in) so a broken download is never stored.

The engine (`UploadWorker`, after file uploads, edits and the state pull, same network rule and
token handling) runs a **history step**:

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
- No limit by date: the whole history comes down. If that turns out too big, add a "since" later.
- Desktop browsers keep using the server only.

## Tests that must exist

- Server: list pagination and order, user isolation (other user's file → not listed, content
  404), token without user 403, content bytes equal the stored blob.
- Embedded/FFI: `missing_files`; `ingest_history_file` stores and derives a Connect envelope and a
  FIT, refuses a sha mismatch, is idempotent.
- Android (JVM tests on the pure parts): paging, the per-pass limit, newest first, backoff and
  the 6-hour rule.
