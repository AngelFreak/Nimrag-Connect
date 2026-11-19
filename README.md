# nimrag

A self-hosted replacement for the Garmin Connect app and cloud, built for one Garmin Instinct 2 and one Android phone.

- An Android app pairs with the watch over Bluetooth LE, pulls its files, and uploads them to a server you run.
- The server keeps every original file untouched (addressed by SHA-256), derives everything else from those files, and serves its own web interface.
- Nothing is sent to Garmin. The only planned exception is optional GPS-assist data, which will be a setting you can turn off.

## Status

Work in progress. See [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md) for the full roadmap and the status of each stage.

| Phase | State |
| --- | --- |
| 0. Protocol map from static analysis and our own Bluetooth captures | done; night-sync and fresh-pairing captures still open |
| 1. Server: ingest, file store, FIT decoding, parsers, `reparse`, export import | done (gate 1 passed on a USB copy of the watch) |
| 2. Android sync | sans-IO protocol client done (replays every captured session); Android app next |
| 3. Web interface | not started |
| 4. Workouts to the watch, backups, export, runbook | not started |

## Layout

| Path | What |
| --- | --- |
| `core/` | Rust crate: sans-IO watch protocol, protocol client, FIT decoding and parsers |
| `server/` | Rust service (axum, SQLite via sqlx): ingest, store, `reparse`, importer |
| `tools/decode/` | CLI over `core` that decodes Bluetooth snoop logs and FIT files |
| `tools/proto-recover/`, `tools/fit-profile/` | generators used during protocol work |
| `docs/protocol/` | the protocol as decoded: `PROTOCOL.md`, message and file-type tables, recovered schemas, open questions |
| `docs/` | architecture and licensing records |

## Building and testing

```sh
cargo test --workspace
```

No test needs the watch. Tests that replay real captures run only when the git-ignored `private/` directory is present, and skip otherwise.

FIT field names come from a profile table generated locally from your own copy of the Garmin Connect app (`tools/fit-profile/`). The table is Garmin material, so it is never committed. Without it, everything builds and raw decoding works, but derived tables (activities, wellness) stay empty. Do not share binaries or images built with a generated profile. See [`docs/LICENSING.md`](docs/LICENSING.md).

## Rules this project follows

- Original files are the source of truth; the database can always be rebuilt from them.
- Never invent data: a metric the watch didn't record is shown as unavailable.
- Protocol work follows evidence. Every finding in `docs/protocol/` is marked as seen, inferred or unknown.
- Gadgetbridge (AGPL) was read as documentation only; no code was copied. See `docs/LICENSING.md`.
- No personal data in the repository. Captures and real files stay in the git-ignored `private/` directory.

## Licence

Not chosen yet. Until a licence is added, all rights are reserved. "Garmin", "Instinct" and "Garmin Connect" are trademarks of Garmin Ltd.; this project is not affiliated with Garmin.
