# nimrag-server

Ingest server for nimrag. Single user, runs behind the owner's VPN, never calls Garmin.

**Files are the source of truth.** Everything under `DIR/files/` is written via
temp file + fsync + rename, streamed to `files/.incoming/` first (64 MiB limit
enforced while streaming; the hash is computed server-side):

- `files/<aa>/<bb>/<sha256>`: the uploaded bytes, untouched.
- `files/<aa>/<bb>/<sha256>.json`: sidecar with `received_at`, the first
  upload's metadata, and one event per upload (device, time, metadata as
  received, `created`), appended under a process-wide lock. One sidecar per
  file, rewritten atomically.

`DIR/nimrag.db` (SQLite, WAL, foreign keys, busy_timeout 10s, synchronous
NORMAL) holds:

- **ledger** (never touched by `reparse`): `files`, `uploads` (mirror of the
  sidecars; `rebuild-ledger` recreates them), `tokens` (database only);
- **derived** (deletable, rebuilt by `reparse`, no autoincrement ids or
  creation times): `file_parse` (parser_version, profile_version, status
  ok/partial/error, error, crc_status), `fit_files`, `fit_messages` (STRICT,
  WITHOUT ROWID, PK `(sha256, chain_idx, msg_idx)`, index `(global, timestamp)`;
  `fields` is a JSON object keyed by field number in definition order with
  `{t, v, raw, be?}`: raw unscaled values, null = invalid, hex for bytes, no
  names or scaling).

Ingest accepts any bytes and never rejects on decode failure; decoder errors
and panics are recorded in `file_parse`. Whether a file is FIT-parsed is decided
from its bytes only (header size >= 12 and `.FIT` at bytes 8..12), never from
metadata; other files get no `file_parse` row. Parsing is gated on the database
(FIT bytes and no `file_parse` row yet), so a retried upload repairs an upload
that failed after the sidecar was written. Identical re-uploads (same device and
metadata as the previous event) increment `repeat_count`/`last_at` on that event
instead of appending, in both the sidecar and `uploads`.

## Usage

```
nimrag-server serve [--data-dir DIR] [--listen 127.0.0.1:8080] [--log-json]
nimrag-server token create --name NAME   # prints the token once on stdout
nimrag-server token list
nimrag-server token revoke ID
nimrag-server reparse                    # derived tables from the blobs
nimrag-server rebuild-ledger             # files/uploads rows from the sidecars
nimrag-server user set-password          # owner password from stdin (first line), argon2id
nimrag-server dev-seed                   # 120 days of sample data into an empty data dir
nimrag-server export-types [--out DIR]   # TypeScript for the JSON API (default web/src/api/generated)
```

`serve` also takes `--insecure-cookies` (see "Owner API"). Set the password
without echoing it, e.g. `printf '%s\n' "$PW" | nimrag-server user set-password`
or `nimrag-server user set-password < pwfile` (at least 10 characters, one line,
at most 4096 bytes). Setting it again replaces it and signs every session out.
`user set-password` does not take the data-dir lock; `dev-seed` does.

`--data-dir` defaults to `$NIMRAG_DATA_DIR`, else `./data`. Logs go to stderr
(`RUST_LOG` filter; `--log-json` for structured JSON). `reparse` processes files
in sha256 order inside a single `BEGIN IMMEDIATE` transaction (clear sample
tables, delete by sha256, reinsert), so sample tables are never visible
half-rebuilt. `reparse` may run while `serve` is up, but it holds the write lock
for its whole run: an upload arriving mid-reparse can fail with 500 after the
10 s busy_timeout, and the client must retry (the retry repairs via the sidecar
path). A missing or corrupt blob is recorded as a `file_parse` error, the
run continues, and the command exits non-zero with the count. `serve` removes
leftovers in `files/.incoming/` at startup. `rebuild-ledger` adds missing `files` rows and replaces each file's
`uploads` with its sidecar events; it never deletes other rows.

API (all but health need `Authorization: Bearer <token>`):

- `POST /api/files?data_type=&sub_type=&file_index=&file_number=&watch_timestamp=&source=watch|usb|import&sha256=`
  (`source` is required; missing gives 400)
  body = raw bytes. Optional `sha256` is verified (400 on mismatch). 201
  `{sha256,size,created:true}` for new bytes, 200 with `created:false` for known bytes, 413 over the limit.
- `GET /api/files/<sha256>`: metadata plus FIT summary.
- `GET /api/health`.

## Owner API (stage 3a)

The web app reads and edits through a second, owner-facing JSON API in the same
router. Authentication is separate from the device tokens:

- `POST /api/login` `{password}` checks the argon2id hash and sets the cookie
  `nimrag_session`: a random 32-byte id, of which only the SHA-256 is stored
  (`sessions`). Flags: `HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000` (30
  days) and `Secure`, unless the server runs with `--insecure-cookies`.
  **Serve it over HTTPS** (the reverse proxy or tailnet terminates TLS): browsers
  do not send a `Secure` cookie over plain HTTP, so without TLS only
  `--insecure-cookies` (local development) works.
- Failed logins are backed off per client address, in memory: the first four are
  free, the fifth starts a lock-out of 2 s that doubles per further failure up to
  300 s (`429` with `Retry-After`); a success resets it. Behind a reverse proxy
  every request has the proxy's address, so the limit is then global, which is
  acceptable for a single owner.
- Every `POST`, `PATCH` and `DELETE` under `/api` except `/api/files` and
  `/api/login` must carry `X-Nimrag: 1` (`403` otherwise), so a cross-site form
  cannot change anything even if a cookie were attached.
- Device tokens only upload (`/api/files`, `/api/health`, file metadata). They
  are rejected on every endpoint below, and a session cookie is rejected on the
  device endpoints.
- Errors are JSON `{"error": "..."}`; malformed bodies are 400 as well.

Conventions: JSON field names are snake_case with the unit as a suffix
(`duration_s`, `distance_m`, `weight_kg`, `avg_hr_bpm`); timestamps are RFC 3339
with the offset of the owner's time zone; dates are `YYYY-MM-DD` local days.
Missing data is `null`, never `0`. `sample: true` marks seeded data.
Settings: `timezone` (IANA name, default the system zone), `step_goal` (8000),
`intensity_goal_min` (150), `home_cards` (`{order, hidden}` of card ids, default
order energy, heart_rate, stress, sleep, hrv, training_status, intensity_minutes,
training_load, steps, recovery_time, vo2_max, respiration, spo2, weight,
training_readiness; the last four start hidden; PATCH rejects unknown ids and
duplicates and appends cards missing from `order`).

| endpoint | what |
|---|---|
| `POST /api/login`, `POST /api/logout`, `GET /api/me` | session |
| `GET /api/tokens`, `POST /api/tokens` `{name}`, `DELETE /api/tokens/{id}` | device tokens for the web UI: the secret is returned once on create, never listed; delete revokes |
| `GET /api/today` | the Today page: In focus (energy, last night, HRV), every At a glance card, last activity, training this week, `yesterday`, `last_7_days` |
| `GET /api/days/{date}` | one day: heart rate, stress, energy, steps, intensity, the night that began that day, activities |
| `GET /api/activities?type=&before=&limit=` | newest first, `type` is all, run, walk, hike, cycle, swim, strength, training, other; `next_before` is the cursor |
| `GET /api/activities/{id}` | detail with laps, HR zones and a thinned columnar series; `id` is the file sha256 |
| `GET /api/activities/{id}/track` | GPS points (at most 1500) with bounds |
| `GET /api/activities/{id}/strength`, `PATCH` | sets grouped by exercise; PATCH `{sets:[{idx, reps?, weight_kg?, exercise?, reset?}]}` stores edits in `strength_set_overrides`, keyed by the activity, so they survive `reparse` and new files of the same activity |
| `GET /api/sleep/{date}`, `GET /api/sleep?nights=7\|14\|30` | a night with stages and vitals; a list of nights |
| `GET /api/trends?range=7d\|4w\|3m\|1y` | HRV with baseline, resting HR, breathing, SpO2, sleep score and duration |
| `GET /api/movement/{date}` | steps, distance, week of steps, intensity week, calories |
| `GET /api/training` | training status, load, load balance, recovery, VO2 max |
| `GET /api/body`, `POST /api/body`, `DELETE /api/body/{id}` | manual weight, body fat and waist entries with the trend |
| `GET /api/calendar/{yyyy-mm}` | per day activities and summary values, month totals |
| `GET /api/devices`, `GET /api/storage` | watch battery, last data and sync; file and parse counts |
| `GET /api/settings`, `PATCH /api/settings` | see above |

Owner data (`owner`, `sessions`, `settings`, `strength_set_overrides`,
`body_measurements`, migration 0008) lives in tables `reparse` never touches.
Interpretation rules that are not obvious from the watch data (cumulative step
counters, weekly intensity totals, night date, VO2 max from MET) are documented
at the top of `src/web_api/daily.rs` and `src/web_api/training.rs`.

TypeScript types for every request and response are generated by ts-rs into
`web/src/api/generated/` (`nimrag-server export-types`). `tests/types.rs` fails
when the committed files differ from a fresh generation.

`dev-seed` writes 120 days of deterministic sample data (GPS runs around a
neutral location, strength sessions with sets, walks, heart rate, steps, stress,
energy, sleep with stages, HRV, resting HR, breathing, SpO2, training load, VO2
max, recovery, battery) and sets `settings.sample_data`, which makes every
response say `sample: true`. It refuses a data dir that holds uploaded files or
was seeded already.

## Building (sqlx offline)

Queries in the ingest and parse code are checked at compile time (the owner API
uses runtime queries, covered by tests instead, and does not touch `.sqlx/`). The checked query data lives in
`server/.sqlx/` and is committed, so normal builds need no database:

```
SQLX_OFFLINE=true cargo build -p nimrag-server     # also: cargo test / clippy
```

After changing any `sqlx::query!` or a migration, regenerate (needs
`cargo install sqlx-cli --no-default-features --features sqlite`):

```
cd server
export DATABASE_URL=sqlite://$PWD/dev.db     # absolute path; dev.db is gitignored
rm -f dev.db*; cargo sqlx database setup     # applies migrations/
cargo sqlx prepare                           # rewrites server/.sqlx/
cargo sqlx prepare --check                   # CI: fail if .sqlx/ is stale
```

`--workspace` would write `.sqlx/` at the workspace root instead; with one
sqlx crate we keep it in `server/` (macros find it via CARGO_MANIFEST_DIR).
`build.rs` re-runs the build when `migrations/` changes. Use
`SQLX_OFFLINE=true` in Docker/CI.

Without `SQLX_OFFLINE=true` and without `DATABASE_URL`, the build fails.

## Dependencies

Versions verified on crates.io on 2026-11-19 (latest stable, not yanked).

| crate | version | why |
|---|---|---|
| axum | 0.8.9 | HTTP router; tokio-team maintained, small API |
| tokio | 1.54.1 | async runtime (axum/sqlx), `spawn_blocking` for file IO and decoding |
| sqlx | 0.9.1 (sqlite, macros, migrate, runtime-tokio; no default features) | compile-time checked queries, embedded migrations, WAL options. 0.9 is a fresh major (2026-05); MSRV 1.94 |
| sha2 | 0.11.1 | SHA-256 for file addressing and token hashes (RustCrypto) |
| serde / serde_json | 1.0.229 / 1.0.154 (`preserve_order`) | request/response, sidecars, and JSON columns in definition order |
| http-body-util | 0.1.5 | stream request bodies frame by frame with a length limit |
| tracing / tracing-subscriber | 0.1.44 / 0.3.23 (json, env-filter) | structured logs, optional JSON output |
| anyhow | 1.0.104 | error context in a binary-style app |
| getrandom | 0.4.3 | OS randomness for 256-bit device tokens (avoids the larger `rand` tree) |
| zip | =9.6.1 (default-features off, `deflate` only) | read the Garmin export archive and the zips nested in it. Maintained (zip-rs/zip2, ~350M downloads, releases through 2026-11-19; 9.7.x was hours old at selection so 9.6.1 is pinned), `enclosed_name()` path-traversal protection built in. Server crate only; core stays std-only |
| argon2 | 0.6.0 | owner password hash (argon2id, RustCrypto); built with opt-level 3 in dev profiles so tests stay fast |
| chrono / chrono-tz | 0.4.45 (std, now) / 0.10.4 | local days and DST-safe day bounds in the owner's IANA time zone |
| iana-time-zone | 0.1.65 | default `timezone` from the system |
| ts-rs | 12.0.1 (no-serde-warnings) | TypeScript types generated from the API structs |
| dev: tower 0.5.3, http-body-util 0.1.5, tempfile 3.27.0 | | drive the real router with `oneshot`, read bodies, temp data dirs |

CLI is hand-rolled (no clap): a handful of fixed subcommands and flags; one fewer
dependency tree to audit.

## Docker

```
docker build -f server/Dockerfile -t nimrag-server .     # from the workspace root
docker run -v nimrag-data:/data -p 8080:8080 nimrag-server
docker run --rm -v nimrag-data:/data nimrag-server token create --name phone --data-dir /data
```

Multi-stage build with `SQLX_OFFLINE=true`; the runtime image runs as a
non-root user with `/data` as a volume.

Testing note: `derive::DERIVE_OVERRIDE` is a test-only seam. Tests that set it
must live in `tests/derive_shared.rs` and hold its lock.

## Importing a Garmin data export

```
nimrag-server import-export --data-dir DIR [--list-skipped]
    [--max-depth N] [--max-entries N] [--max-total-bytes N] <archive.zip | extracted-dir>
```

Reads files only (no network). Walks the zip recursively (zips inside zips) and
sends every entry whose bytes look like FIT through the same `ingest::ingest` the
HTTP upload uses, with `source = import`. The archive-relative path is recorded
as `origin` on the sidecar upload event and in the `uploads.origin` column.

- Limits (defaults: depth 4, 2M entries, 64 GiB materialised bytes in total, 64
  MiB per FIT entry) are enforced on bytes actually read, not on declared sizes.
  Nested zips are spooled to temp files in `<data>/files/.incoming/` (removed
  afterwards), so the data dir needs room for the largest inner zip.
- Names are metadata only: blobs are stored by hash and a name never touches the
  filesystem. Absolute or `..` names are therefore imported, with the raw name as
  `origin`, and reported as "unsafe names (imported, name kept as metadata only)".
- Idempotent by hash: a re-run only bumps the repeat counter of the matching event.
  Re-importing the same export extracted differently (directory vs zip, different
  root) has different origins, so it adds one new event per file.
- A failed entry is reported and the rest carries on; the exit is non-zero if any
  entry errored or a global limit aborted the run.
- Not stored in this stage (counted and listed by extension): TCX, GPX, JSON and
  anything not FIT by content. Open item: the JSON wellness summaries are
  Garmin-computed cloud data, not watch files; the owner decides later whether to
  import them.

`serve`, `import-export`, `reparse`, `rebuild-ledger` and `dev-seed` take an exclusive lock on
`<data>/.lock`; a second one fails fast with "data dir ... in use by `<command>`".
(`token` and `user` do not lock.) This needs Rust 1.89 for `File::try_lock`, so the server
crate sets `rust-version = "1.89"`; core stays at 1.80.
