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
```

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

## Building (sqlx offline)

Queries are checked at compile time. The checked query data lives in
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
| dev: tower 0.5.3, http-body-util 0.1.5, tempfile 3.27.0 | | drive the real router with `oneshot`, read bodies, temp data dirs |

CLI is hand-rolled (no clap): five fixed subcommands and three flags; one fewer
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
