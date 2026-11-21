# nimrag-server

Ingest server for nimrag. Several users (one admin who sets the server up, normal users who own the
data), runs behind the owner's VPN, never calls Garmin (except the opt-in Forward mode).

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
  sidecars; `rebuild-ledger` recreates them), `tokens` (database only),
  `file_owners` (whose file a sha256 is; database only, so `rebuild-ledger` from sidecars
  yields unowned files that an admin assigns again);
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
                    [--insecure-cookies] [--trusted-proxy CIDR[,CIDR...]] [--tiles-dir DIR] [--no-gps-assist]
                    [--backup-dir DIR [--backup-every 24h] [--backup-keep 7]]
nimrag-server token create --name NAME [--user USERNAME]   # for tools and scripts; phones get theirs by signing in to the app
nimrag-server token list
nimrag-server token revoke ID
nimrag-server import-export [--user USERNAME] <archive.zip | dir>   # a Garmin data export (see below)
nimrag-server import-connect --user USERNAME [--timezone IANA] <dir> # Garmin Connect JSON history (see below)
nimrag-server reparse                    # derived tables from the blobs
nimrag-server rebuild-ledger             # files/uploads rows from the sidecars
nimrag-server backup --out DIR [--keep N]    # one backup now, safe while serve runs (see "Backups")
nimrag-server restore --from BACKUP --data-dir NEWDIR   # a backup into a new, empty data dir
nimrag-server export --user USERNAME --out FILE.zip     # one user's originals and CSV files (see "Export")
nimrag-server user list                  # accounts (no secrets)
nimrag-server user set-password [--username NAME]   # new password from stdin (first line), argon2id
nimrag-server user reset-2fa --username NAME        # the user enrols 2FA again at the next login
nimrag-server dev-seed                   # 120 days of sample data, an admin and the user demo, into an empty data dir
nimrag-server export-types [--out DIR]   # TypeScript for the JSON API (default web/src/api/generated)
```

`serve` also takes `--insecure-cookies` and `--trusted-proxy` (see "Owner API") and `--tiles-dir`
(see "Map tiles"). The first admin
is created in the web app on first run (see "Accounts, 2FA and the app"); the CLI user commands
are the recovery tools of whoever runs the server. Set a password without echoing it, e.g.
`printf '%s\n' "$PW" | nimrag-server user set-password --username NAME` or
`nimrag-server user set-password --username NAME < pwfile` (at least 10 characters, one line, at
most 4096 bytes; without `--username` the only admin is meant, and with several admins the name is
required). It signs the user's sessions and trusted phones out. `token create --user` and
`import-export --user` give the token or the imported files to that user; without `--user` they
stay unassigned until an admin assigns them in the web app. The `user` and `token` commands do not
take the data-dir lock; `dev-seed` does. `backup` and `export` only read and do not take it either;
`restore` locks the new dir it fills.

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

A device token belongs to one user (`tokens.user_id`; NULL = unassigned). The files it uploads are
owned by that user (`file_owners`, first claim wins: the same bytes from a second user do not take
the file over). `GET /api/files/<sha256>` only answers for files the token's own user owns.

## Accounts, 2FA and the app

Two roles. The **admin** sets the server up and manages accounts; the admin has no data and gets
`403` on every data endpoint. **Users** are created by the admin and own all data (files, tokens,
settings, body entries, strength edits, Garmin Forward). The first admin is created in the web
app on first run:

- `GET /api/setup` `{needed}`; `POST /api/setup` `{username, password}` (only while no user exists)
  answers `{pending_token, otpauth_uri, secret}`; `POST /api/setup/confirm` `{pending_token, code}`
  with a code from the authenticator app creates the admin and signs in. Setup closes only once the
  code is confirmed, so an abandoned attempt leaves the server open for a new one; two racing
  attempts cannot both succeed. Usernames are 3 to 32 characters of `a-z 0-9 . _ -`, stored
  lower case (input is case-insensitive); passwords are at least 10 characters (argon2id).
- **Two-factor** is TOTP (RFC 6238, SHA-1, 6 digits, 30 s, one step of drift either way). A code is
  accepted once: the used step is stored with a compare-and-set, so replays and parallel uses fail.
  Implemented with `totp-rs` (alloc only); the secret and the `otpauth://` URI come from the server
  and the web app draws the QR code, so no secret leaves the server except in the enrol response.
- `POST /api/login` `{username, password, code?}`. Unknown user, wrong password, wrong or missing
  code, disabled user: all the same `401 {"error": "wrong username, password or code"}`, and an
  unknown user still pays for a (dummy) password hash. A right password for a user without 2FA
  continues with `{step: "enrol2fa", pending_token, otpauth_uri, secret, must_change_password}`:
  show the QR code and send a code (and, with `must_change_password`, the new password) to
  `POST /api/login/totp-enrol`. A temporary password with 2FA already set answers
  `{step: "change_password", pending_token}` for `POST /api/login/change-password`. Success is
  `{step: "done", ...Me}` and the session cookie. Pending steps live in memory for 10 minutes and
  burn after 5 wrong codes. Note the consequence of enrolling at first login: a user without 2FA
  (a fresh account, or the migrated admin) can enrol with the password alone, so hand new users
  their temporary password personally.
- Limits: the per-address limiter described below, plus a per-username one (nine free failures,
  then the same doubling wait), so spreading guesses over many addresses does not help; a missing
  code counts as a failure.
- `GET /api/me` `{authenticated, username, role, timezone, sample}`; `POST /api/logout`;
  `POST /api/me/password` `{current_password, new_password}` (ends every session and trusted
  phone, then signs this browser in again); `POST /api/me/2fa` `{password}` +
  `POST /api/me/2fa/confirm` `{pending_token, code}` replace the authenticator.
- **Admin API** (`403` for users, `401` signed out): `GET/POST /api/admin/users`
  (`{username, role?}` answers the user and a `temporary_password`, shown once; the user must
  enrol 2FA and choose a password at the first login), `POST /api/admin/users/{id}/reset-password`
  (a new temporary password), `.../reset-2fa`, `.../disable`, `.../enable`,
  `DELETE /api/admin/users/{id}` (`409` while the user has data unless `?confirm=true`; their
  files and tokens become unassigned, nothing on disk is deleted, their records are removed).
  Reset-password, reset-2fa, disable and delete end the user's sessions and trusted-phone
  credentials. The last active admin cannot be disabled or deleted (`409`), and the admin cannot
  reset their own password or 2FA there (`400`: use their Profile page or the CLI).
- **Unassigned data**: `GET /api/admin/unassigned` lists active tokens nobody owns (with the
  number of unowned files each uploaded), `unattributed_files` (unowned files no such token
  uploaded) and `legacy` (settings, body entries, strength edits, Garmin account that predate
  accounts). `POST /api/admin/unassigned/assign` `{token_ids, user_id, include_unattributed?,
  adopt_legacy?}` moves those tokens and their files to the user in one transaction and rebuilds
  the derived rows of both owners. `adopt_legacy` is refused (`409`) when the user already has
  records of their own, so nothing is silently merged.

### Migration from the single-owner server

Migration 0010 is lossless. The old owner becomes user `admin` (role admin, the same argon2id hash,
2FA not enrolled: the first login enrols it, then sign-in is username + password + code).
Sessions are cleared. Existing device tokens, files and sample rows are unassigned (`user_id`
NULL/0) and keep working as unassigned uploaders; settings, body entries, strength edits and the
Garmin account get `user_id` 0. The admin creates the user (for example the old owner's daily
account), then assigns the old tokens (and `adopt_legacy`) to that user in `/admin`. Because the
admin has no data, the web app's data pages stay empty until then.

### The Android app

`X-Nimrag: 1` is required as for the web UI. Three calls, with the same limiter and the same
indistinguishable `401`:

- `POST /api/app/login` `{username, password, code, device_name}`: the first time on a phone.
  `201 {me, device_id, token, device_secret}` and a session cookie. `token` is a normal device
  token (upload credential) and `device_secret` (`nrd_...`, 256 random bits) is the trusted-device
  credential; both are shown once and only their SHA-256 is stored. **This is the only way a user
  gets a device token**: the app keeps it out of sight, nobody types, pastes or copies one, and the
  web UI cannot create one (`POST /api/tokens` is gone). The app cannot enrol 2FA
  (a user without 2FA gets `401`), admins get `403`, a temporary password `409` (change it in the
  web app first), and the 20-active-token cap applies (`409`).
- `POST /api/app/unlock` `{username, password, device_secret}`: later, on the same phone, the
  password plus the secret starts a session; the secret stands in for the 2FA code.
- `POST /api/app/resume` `{device_secret}`: the fingerprint path. The app keeps `device_secret` in
  the Android Keystore behind a BiometricPrompt (`setUserAuthenticationRequired`). The server cannot
  check a fingerprint, so the gate is on the phone and the 256-bit secret is itself the credential:
  a bare `{biometric: true}` is not accepted, and neither is anything but the secret. Guessing is
  limited per address (`429` with `Retry-After`).

Revoking the device token (web Profile, "Signed-in phones", "Sign out", or the CLI)
removes its device secret and ends the sessions it started, so the phone must sign in with
username, password and code again. A phone whose upload comes back `401`/`403` forgets its
credentials the same way and shows the sign-in. Disabling
the user, resetting their password or 2FA, or deleting them does the same for every device of
that user.

## Owner API (stage 3a)

The web app reads and edits through a second JSON API in the same router, for the signed-in user
(everything below is filtered by that user; sign-in and the admin are described under "Accounts,
2FA and the app"). Authentication is separate from the device tokens:

- `POST /api/login` checks username, password and code and sets the cookie
  `__Host-nimrag_session`: a random 32-byte id, of which only the SHA-256 is
  stored (`sessions`). Flags: `HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000`
  (30 days) and `Secure`. With `--insecure-cookies` the `Secure` flag is dropped
  and the name is plain `nimrag_session` (the `__Host-` prefix is invalid
  without `Secure`); only the name matching the mode is accepted.
  **Serve it over HTTPS** (the reverse proxy or tailnet terminates TLS): browsers
  do not send a `Secure` cookie over plain HTTP, so without TLS only
  `--insecure-cookies` (local development) works.
- Failed logins are backed off per client address, in memory: the first four are
  free, the fifth starts a lock-out of 2 s that doubles per further failure up to
  300 s (`429` with `Retry-After`); a success resets it, and so do 15 minutes
  without a failure. On top of that, more than 20 failures within a minute across
  all addresses make every login wait 1 s (fixed) after each further failure. Password
  checks run one at a time and re-check the limiter when their turn comes, so a
  burst of parallel guesses cannot all slip through.
- **Behind a reverse proxy** the TCP peer is the proxy, so by default all clients
  share one limit. `serve --trusted-proxy CIDR` (repeatable, or comma-separated,
  e.g. `--trusted-proxy 127.0.0.1/32,172.16.0.0/12`) opts in to
  `X-Forwarded-For`: only when the TCP peer is inside a trusted network, the
  rightmost entry that is not itself trusted is the client address (entries further
  left are client-supplied and ignored). From any other peer the header is ignored.
  Trust only networks your proxy really sits in, and make the proxy append the
  address it saw rather than pass a client's header through unchecked.
- Body limits: `POST /api/login` 4 KiB, every other JSON route 64 KiB (`413`),
  `POST /api/files` (device upload) the large upload limit.
- Every response under `/api` carries `Cache-Control: no-store`,
  `X-Content-Type-Options: nosniff` and `Vary: Cookie`.
- Every `POST`, `PATCH` and `DELETE` under `/api` except `/api/files` and
  `/api/login` must carry `X-Nimrag: 1` (`403` otherwise), so a cross-site form
  cannot change anything even if a cookie were attached.
- Device tokens only upload (`/api/files`, `/api/health`, file metadata of their own user's
  files). They are rejected on every endpoint below, and a session cookie is rejected on the
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
| `/api/login`, `/api/logout`, `/api/me`, `/api/setup`, `/api/me/password`, `/api/me/2fa`, `/api/app/*`, `/api/admin/*` | accounts: see "Accounts, 2FA and the app" |
| `GET /api/tokens`, `DELETE /api/tokens/{id}` | the signed-in user's own phones (name, created, last used, active, revoked; never a token value): the web Profile lists them as "Signed-in phones" (`current` is true for the phone the request's session was started by, so the page can say "this phone"; false in a browser session) and signs one out (delete revokes the token, its device secret and its sessions; another user's id is `404`). Tokens are issued only by the app's sign-in (at most 20 active per user, `409` beyond), not by this API |
| `GET /api/about` | `{ version }`: the server's version, for the Profile page's About row; any signed-in session (user or admin), `401` otherwise |
| `GET /api/today` | the Today page: In focus (energy, last night, HRV), every At a glance card, last activity, training this week, `yesterday`, `last_7_days` |
| `GET /api/days/{date}` | one day: heart rate, stress, energy, steps, intensity, breathing and blood oxygen (day figures and means per hour), the night that began that day, activities |
| `GET /api/activities?type=&before=&limit=` | newest first, `type` is all, run, walk, hike, cycle, swim, strength, training, other; `next_before` is the cursor |
| `GET /api/activities/{id}` | detail with laps, HR zones and a thinned columnar series; `id` is the file sha256 |
| `GET /api/activities/{id}/track` | GPS points (at most 1500) with bounds |
| `GET /api/activities/{id}/strength`, `PATCH` | sets grouped by exercise; PATCH `{sets:[{idx, reps?, weight_kg?, exercise?, reset?}]}` stores edits in `strength_set_overrides`, keyed by the activity, so they survive `reparse` and new files of the same activity |
| `GET /api/sleep/{date}`, `GET /api/sleep?nights=7\|14\|30` | a night with stages and vitals; a list of nights |
| `GET /api/trends?range=7d\|4w\|3m\|1y` | HRV with baseline, resting HR, breathing, SpO2, sleep score and duration |
| `GET /api/metrics/{metric}?range=7d\|4w\|12w\|6m\|1y&end=YYYY-MM-DD` | a metric detail page's period (`heart_rate`, `stress`, `energy`, `steps`, `intensity_minutes`, `training_load`, `vo2_max`, `respiration`, `spo2`): rolling windows of 7, 28, 84, 182 and 364 days ending on `end`; `series` of `points` per bucket (`day` over 7d and 4w, `week`, 7 days, over 12w, 6m and 1y; intensity minutes use calendar weeks: `7d` is the Monday to Sunday week holding `end`, weekly buckets start on Mondays), summary `stats`, `goal` and training-load `target_low`/`target_high`, `has_prev`/`has_next`. Unknown metric 404, bad `range` or `end` 400; `end` defaults to and is capped at today; a bucket without data is `null` |
| `GET /api/movement/{date}` | steps, distance, week of steps, intensity week, calories |
| `GET /api/training` | training status, load, load balance, recovery, VO2 max |
| `GET /api/training/status?from&to` | the Training status page: `days` (one status per local day), `runs` (consecutive equal statuses, newest first, with `ongoing` and `starts_before_range`), the colour `legend`, and today's `current` status and `load`. Each status is `{code, label, kind}`; only codes 1 Detraining, 4 Maintaining, 5 Recovery, 7 Productive, 8 Strained and 0 No status are verified, any other code is sent as `Status N` with kind `unknown` |
| `GET /api/hrv?from&to` | the HRV status page: one entry per local day (status, 7-day average, overnight average, balanced range), the mean of the overnight values, the latest range, and for a one-day range the night's readings |
| `GET /api/sleep/history?from&to&group=day\|week\|month` | the Sleep score page for more than one night: buckets with average, highest and lowest score and average time asleep, and the stats over the whole range (`long_night_s` is 8 h) |
| `GET /api/body`, `POST /api/body`, `DELETE /api/body/{id}` | manual weight, body fat and waist entries with the trend |
| `GET /api/calendar/{yyyy-mm}` | per day activities and summary values, month totals |
| `GET /api/devices`, `GET /api/storage` | watch battery, last data and sync; file and parse counts |
| `GET /api/settings`, `PATCH /api/settings` | see above |

Owner data (`users`, `sessions`, `user_settings`, `strength_set_overrides`,
`body_measurements`, `file_owners`, migrations 0008 and 0010) lives in tables `reparse` never
touches. Every derived sample table has `user_id` in its primary key (0 = unassigned); the
`*_current` views carry the owner through `file_owners`, and a by-id endpoint (activity, track,
strength) answers 404 for a file the session's user does not own.
`reparse` re-derives from the files and keeps `file_owners`.
The three range endpoints take local dates `from` and `to` (`to` defaults to today and is
clamped to it, `from` to `to` minus 6 days; a range over 400 days or with `from` after `to` is
a 400). They read the derived tables (`training_status`, `training_load`, `hrv_summary`,
`hrv_values`, `sleep_sessions_current`), so rows that a later Garmin Connect import adds show
up without changes, and every query filters by the session's user.
Interpretation rules that are not obvious from the watch data (cumulative step
counters, weekly intensity totals, night date, VO2 max from MET) are documented
at the top of `src/web_api/daily.rs` and `src/web_api/training.rs`.

TypeScript types for every request and response are generated by ts-rs into
`web/src/api/generated/` (`nimrag-server export-types`). `tests/types.rs` fails
when the committed files differ from a fresh generation.

`dev-seed` creates two accounts, the admin `admin` (no data) and the user `demo`, both with 2FA
enrolled, and gives `demo` 120 days of deterministic sample data (GPS runs around a
neutral location, strength sessions with sets, walks, heart rate, steps, stress,
energy, sleep with stages, HRV, resting HR, breathing, SpO2, training load, VO2
max, recovery, battery) and sets `settings.sample_data`, which makes every
response say `sample: true`. It refuses a data dir that holds uploaded files or
was seeded already. Passwords and 2FA secrets are random unless given in
`NIMRAG_SEED_ADMIN_PASSWORD`, `NIMRAG_SEED_ADMIN_TOTP`, `NIMRAG_SEED_USER_PASSWORD` and
`NIMRAG_SEED_USER_TOTP` (base32); the generated ones are printed once on stdout for whoever ran
the command, the supplied ones never, and nothing goes to the logs.

## Workouts (stage 4a)

Workouts are built on the web, saved here, written as a FIT workout file when the phone asks for
it, and sent to the watch by the Android app (file type 128/5). Migration `0013_workouts.sql`; code
in `src/web_api/workouts.rs`; tests in `tests/workouts.rs`.

Owner side (session cookie; writes need `X-Nimrag: 1`; an admin gets `403`; a workout of another
user is `404`):

| endpoint | what |
|---|---|
| `GET /api/workouts` | `{watch_send: {available, reason}, limits, workouts: [{id, name, sport, steps, duration_s, updated_at, send}]}`, newest change first. `duration_s` is the total when every step has a time (repeats counted), else `null`; `steps` counts every step, those in repeat blocks included |
| `POST /api/workouts` | create from a body (below); `201` with the detail; at most 200 per user (`409`) |
| `GET /api/workouts/{id}` | `{id, revision, created_at, updated_at, workout, send}` |
| `PUT /api/workouts/{id}` | replace the body; `revision` + 1. A queued workout stays queued (the phone sends the new revision); `sent` and `failed` go back to `idle` |
| `DELETE /api/workouts/{id}` | `204`; a copy already on the watch stays there |
| `POST /api/workouts/{id}/send` | "Send to watch": queue it. `503` with the reason when this server has no FIT profile; `400` with the reason when the workout cannot be written |
| `DELETE /api/workouts/{id}/send` | take it out of the queue again (back to `idle`) |

The body is `{name, sport, note?, nodes}`. `sport` is `running`, `cycling`, `walking`, `hiking`,
`strength` or `other`. A node is a step `{type: "step", intensity, duration, target, note?}` or a
repeat block `{type: "repeat", count, steps: [step without type]}`. `intensity` is `warmup`,
`active`, `rest`, `cooldown` or `recovery`. `duration` is `{kind: "open"}`,
`{kind: "time", seconds}`, `{kind: "distance", meters}`, `{kind: "heart_rate_below", bpm}` or
`{kind: "heart_rate_above", bpm}`. `target` is `{kind: "none"}`, `{kind: "heart_rate_zone", zone}`,
`{kind: "heart_rate_range", low_bpm, high_bpm}`, `{kind: "pace", fast_s_per_km, slow_s_per_km}`,
`{kind: "speed", low_kmh, high_kmh}` or `{kind: "cadence", low, high}`. The limits (also sent in
`limits`, so the builder keeps to the same ones): 100 steps in the file (a repeat block counts one
more than its steps), 1 to 20 steps in a block, 2 to 99 repeats, a name of 1 to 63 bytes, a note
of at most 254 bytes, a step of 1 s to 24 h or 1 m to 500 km, heart rates 30 to 240 bpm, zones 1
to 5, pace 1:00 to 60:00 per km, speed 0.5 to 100 km/h, cadence 1 to 300; low never above high.
The first refused value is named in the `400` text.

`send` is `{state, requested_at, finished_at, error}` with `state` `idle`, `queued`, `sent` or
`failed`. Only `failed` has an `error`: a fixed sentence from the phone (which stage the watch
refused, with Gadgetbridge's status name; never the workout's content).

Phone side (a device token, `Authorization: Bearer`, like `/api/files`; no CSRF header; the token
must belong to a user; a cookie is rejected here):

| endpoint | what |
|---|---|
| `GET /api/device/workouts` | `{workouts: [{id, name, revision}]}`: the user's queued workouts, oldest request first |
| `GET /api/device/workouts/{id}/fit` | the FIT file (`application/octet-stream`), made now from the stored body; header `X-Nimrag-Revision` says which revision it was made from. `404` when the workout is not queued (any more); `503` without the profile |
| `POST /api/device/workouts/{id}/result` | `{revision, ok, error?}` (error cut at 200 characters). Answers `{applied}`: a result for a revision that is no longer the current one is ignored (`applied: false`), so an edit made while the phone was sending is not marked as sent |

**The FIT profile.** The server writes the file with `nimrag_core::fit` and the locally generated
FIT profile (Garmin data, never redistributed, `docs/LICENSING.md`). A server built without it
(the default, `NIMRAG_FIT_PROFILE=none`, and the Docker image) can keep workouts but cannot send
them: `GET /api/workouts` says `watch_send.available: false` with the reason, the web page shows it
before anyone tries, and `send` and the FIT download answer `503` with
"this server was built without the FIT profile, so it cannot write workout files for the watch;
workouts can still be saved here". The phone never needs the profile: it only carries the bytes,
which is why the public Android build can send workouts. The file is never stored: it is made when
the phone fetches it.

Workouts are the user's own data: they are in the export (`csv/workouts.csv`) and removed with the
user. The `X-Nimrag` rule is unchanged for the owner side; `/api/device/` is exempt like
`/api/files`.

## Weather for the watch (stage 2f)

When the phone's "Weather" feature is on, the watch can ask for a forecast (message 5014). The phone
asks this server, the server gets the forecast from [Open-Meteo](https://open-meteo.com/) and writes
it as the FIT `weather_conditions` records the watch expects; the phone only carries the bytes.
Code in `src/weather.rs`; tests in `tests/weather.rs` (a local fake Open-Meteo; no test reaches the
real one) and the unit tests in the module.

Phone side (a device token, `Authorization: Bearer`, no CSRF header; the token must belong to a
user):

| endpoint | what |
|---|---|
| `POST /api/device/weather` | body `{lat, lon, hours?, format?}`. Answers `{definitions, records, fetched_at}`: the FIT definition records and each data record in base64 and the time the forecast was fetched (Unix seconds). `400` for a position that is not on the globe, `401` without a device token, `403` for a token without a user, `502` when Open-Meteo did not answer (10 s timeout), `503` when weather is switched off or this build has no FIT profile |

`hours` (0 or missing means 12, at most 12) sets the number of hourly records. The answer holds one
"current" record, up to 12 hourly ones and 5 daily ones. `format` is the watch's format byte and is
accepted and ignored.

**Privacy.** The watch position is rounded to 0.1 degree (about 10 km) on the phone's core, and
again here. Only the rounded cell goes to Open-Meteo, with this server's address and nothing about
the user or the watch. A position is never stored and never logged, not even rounded: the log line
says `weather answered` with whether the cache was used and the number of records. Error words
from the upstream are fixed (`timeout`, `unreachable`, a status number, `bad answer`) because an
HTTP client's own error text contains the URL, and the URL contains the cell. Answers are cached in
memory (not on disk) for 30 minutes per cell, 256 cells at most, so one cell is asked for at most
twice an hour however often the watch asks, and the cache is gone at a restart. The one place the
cell appears again is the "current" record's observed location, which goes back to the watch.

**Switches.** `NIMRAG_WEATHER=off` or `--weather off` switches the endpoint off (`503`, Open-Meteo
is never contacted); the default is on, but nothing happens until a phone switches the feature on.
`NIMRAG_WEATHER_URL` / `--weather-url` replace the Open-Meteo address (for your own instance, and
for the tests).

**What is mapped.** Temperature, apparent temperature, humidity, wind speed and direction, rain
probability, condition and the day's high and low; dew point, UV index and air pressure when the
FIT profile has the field. Open-Meteo's WMO weather codes become Garmin conditions with our own
table, written out at the top of `src/weather.rs` (clear, partly cloudy, cloudy, fog, light rain,
rain, heavy rain, scattered showers, wintry mix, light snow, snow, heavy snow, thunderstorms; an
unknown code gives no condition). There is no place name and no air quality.

**The FIT profile.** Like workouts, the file is made with `nimrag_core::fit` and the locally
generated FIT profile (Garmin data, never redistributed, `docs/LICENSING.md`), so a server built
without it (the default, `NIMRAG_FIT_PROFILE=none`, and the Docker image) answers `503` with "this
server was built without the FIT profile ...", and the phone then sends the watch nothing. The
phone never needs the profile, which is why the public Android build can carry weather.

**Attribution.** Weather data by [Open-Meteo.com](https://open-meteo.com/), licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Open-Meteo's free service is for
non-commercial use; a self-hosted household server fetching a few forecasts an hour is well inside
it, but read their [terms](https://open-meteo.com/en/terms) if you run it for others.

## GPS assist (stage 2f)

The watch can find GPS faster with satellite orbit data (Garmin's "ephemeris"). The phone asks this
server for it; the server fetches it from Garmin; the phone never talks to Garmin. That one request
is the only contact Nimrag has with Garmin, so it has an off switch: `NIMRAG_GPS_ASSIST=off` (also
`0`, `false`, `no`) or `serve --no-gps-assist`. On otherwise; any other value stops the server at
start. The phone asks only when its own "GPS assist" switch (off by default) is on. `src/gps_assist.rs`.

| endpoint | what |
|---|---|
| `GET /api/device/gps-assist?path=<path?query>` (device token) | the file Garmin serves at `https://api.gcs.garmin.com<path>`; `path` is URL-encoded and must start with `/ephemeris/`. Headers: `ETag` (Garmin's), `Age` (seconds since the copy was fetched), `Content-Type: application/octet-stream`. `If-None-Match` with that tag (a list, `W/` and `*` count) gives `304` |

Errors: `400` for a path outside the whitelist (only letters, digits and `/ . _ - , = & ? % +`, at
most 512 bytes, one `?`, no `..`, no `//`, no `%2e` or `%2f`; the rule is the core's
`is_safe_path_and_query`), `401` without a device token, `404` when the switch is off or Garmin has
no such file, `502` when Garmin does not answer or answers wrongly. The host is fixed in the server;
the phone cannot choose it.

* **Fetch**: `reqwest`, 20 s timeout, no redirects, body cut at 1 MiB (refused, not truncated).
  One fetch at a time. The server does not check the file's contents; the Rust core of the phone
  does (tar of RxNetworks or Sony CPE data) before the watch sees it.
* **Cache**: one pair of files per distinct path in `<data dir>/gps-assist/` (`<sha256 of the path>.bin`
  and `.json` with the tag and fetch time; the path and its query are not stored), fresh for 4 hours
  (Garmin's `max-age=14400`), then revalidated with `If-None-Match` and replaced when the tag changed.
  At most 32 paths are kept. It is a cache: it is not in backups and can be deleted at any time.
  The API-wide `Cache-Control: no-store` stays on the response, so the phone applies the 4 h rule itself.
* **Log**: only "fetched N bytes" and failures, never the path, its query or a token.
* The file seen once by hand: `GET /ephemeris/cpe/sony/lle?coverage=WEEKS_1&constellations=GPS,GLONASS,GALILEO,QZSS`
  answers `200` without login, a 66 KB `application/x-tar` with `CPE_GPS.BIN`, `CPE_GAL.BIN`,
  `CPE_GLO.BIN`, `CPE_QZSS.BIN`, an `etag` and `cache-control: max-age=14400`. The exact URL the
  Instinct 2 asks for is not known yet (`docs/protocol/OPEN-QUESTIONS.md` Q-COMP-1), which is why
  any `/ephemeris/` path is passed on.

Tests: `tests/gps_assist.rs` (a fake Garmin on 127.0.0.1; fetch, cache, 304, revalidation, switch,
token, whitelist, failures, the cap, redirects) and the unit tests in `src/gps_assist.rs`.

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
| tokio | 1.54.1 | async runtime (axum/sqlx), `spawn_blocking` for file IO and decoding; `fs` and `io-util` for the seeks that serve map tiles |
| tokio-util | 0.7.20 (`io` only) | `ReaderStream`: streams a byte range of a tile file without loading the file |
| sqlx | 0.9.1 (sqlite, macros, migrate, runtime-tokio; no default features) | compile-time checked queries, embedded migrations, WAL options. 0.9 is a fresh major (2026-05); MSRV 1.94 |
| sha2 | 0.11.1 | SHA-256 for file addressing and token hashes (RustCrypto) |
| serde / serde_json | 1.0.229 / 1.0.154 (`preserve_order`) | request/response, sidecars, and JSON columns in definition order |
| http-body-util | 0.1.5 | stream request bodies frame by frame with a length limit |
| tracing / tracing-subscriber | 0.1.44 / 0.3.23 (json, env-filter) | structured logs, optional JSON output |
| anyhow | 1.0.104 | error context in a binary-style app |
| getrandom | 0.4.3 | OS randomness for 256-bit device tokens (avoids the larger `rand` tree) |
| zip | =9.6.1 (default-features off, `deflate` only) | read the Garmin export archive and the zips nested in it. Maintained (zip-rs/zip2, ~350M downloads, releases through 2026-11-19; 9.7.x was hours old at selection so 9.6.1 is pinned), `enclosed_name()` path-traversal protection built in. Server crate only; core stays std-only |
| argon2 | 0.6.0 | password hash (argon2id, RustCrypto); built with opt-level 3 in dev profiles so tests stay fast |
| totp-rs | 6.0.0 (default features off, `alloc`) | TOTP codes (RFC 6238, SHA-1, 6 digits, 30 s); the replay guard and the secret generation are ours |
| chrono / chrono-tz | 0.4.45 (std, now) / 0.10.4 | local days and DST-safe day bounds in the owner's IANA time zone |
| iana-time-zone | 0.1.65 | default `timezone` from the system |
| ts-rs | 12.0.1 (no-serde-warnings) | TypeScript types generated from the API structs |
| dev: tower 0.5.3, http-body-util 0.1.5, tempfile 3.27.0 | | drive the real router with `oneshot`, read bodies, temp data dirs |

CLI is hand-rolled (no clap): a handful of fixed subcommands and flags; one fewer
dependency tree to audit.

## Map tiles

```
nimrag-server serve --tiles-dir DIR        # or NIMRAG_TILES_DIR=DIR
```

The web app's maps (the activity route, `web/src/components/MapView.tsx`) read a PMTiles vector
file from `/tiles/map.pmtiles` with HTTP range requests and draw the route alone while that is a
404. `--tiles-dir DIR` (env fallback `$NIMRAG_TILES_DIR`, like `--data-dir`'s) serves the
`*.pmtiles` files of `DIR` as `/tiles/<name>.pmtiles`; the web app asks for `map.pmtiles`, so
that is the file to put there. Without the option the route does not exist and `/tiles/*` is a
404. `src/tiles.rs`:

* **Signed-in users only.** The session cookie (either role, like the rest of the app) is
  required: 401 without one, and a device token does not count. (The tiles are OpenStreetMap data
  and could be public; they are not, so an open server does not serve hundreds of MB to anyone
  who finds it. The web app's request carries the cookie, so nothing changes for it.)
* `GET` and `HEAD`. Single ranges: `Range: bytes=a-b`, `bytes=a-` and `bytes=-n` give 206 with
  `Content-Range: bytes a-b/size`; a range that starts past the end (or `bytes=-0`, or several
  ranges at once, which are not supported) gives 416 with `Content-Range: bytes */size`; a `Range`
  of another unit or one that is not well formed is ignored (200, whole file). Every response
  has `Accept-Ranges: bytes`, `Content-Type: application/octet-stream`, `ETag` (size and
  modification time) and `Last-Modified`; `If-Match` (412), `If-None-Match` / `If-Modified-Since`
  (304) and `If-Range` work, so the PMTiles library notices when the file is replaced.
  `Cache-Control: private, no-cache`.
* Files are opened and read with a seek to the range's start, streamed for its length: a file
  of any size costs a few KB of memory per request (a test seeks 5 GiB into a sparse file).
* Plain names only: letters, digits, `-`, `_` and `.`, ending in `.pmtiles`, not starting with a
  dot. Anything else (`../x`, `%2f`, subdirectories, other extensions, directories) is a 404, as
  is a missing file or directory (the server logs a warning at start when `DIR` cannot be read).
  A symbolic link inside `DIR` is followed: it is the admin's own directory.

Make a tile file for your area with the `pmtiles` CLI (go-pmtiles, <https://github.com/protomaps/go-pmtiles>)
from a Protomaps daily build of OpenStreetMap (`<YYYYMMDD>` is the date of a recent build):

```
pmtiles extract https://build.protomaps.com/<YYYYMMDD>.pmtiles map.pmtiles --bbox=<w,s,e,n> --maxzoom=15
```

`--bbox` is `west,south,east,north` in degrees (for example `8.0,54.5,12.8,57.8` is Denmark);
`--maxzoom=15` keeps a region the size of a country to a few hundred MB. Put `map.pmtiles` in the
tiles directory; a new file replaces the old one without restarting the server. In Docker mount
the directory read-only and set `NIMRAG_TILES_DIR` (see the commented example in
`docker-compose.yml`); the image's user (uid 10001) must be able to read the files.
Map data: © OpenStreetMap contributors (the web app shows the attribution).

Tests: `tests/tiles.rs` (whole file, ranges, HEAD, 416, validators, traversal, missing, sign-in,
a seek deep in a sparse 5 GiB file) and the unit tests in `src/tiles.rs`.

## Backups

Files are the source of truth, but who owns a file, the accounts, the tokens and every setting live
only in `nimrag.db`. A backup therefore holds both.

```
nimrag-server serve --backup-dir DIR [--backup-every 24h] [--backup-keep 7]   # or NIMRAG_BACKUP_DIR,
                                                                              # NIMRAG_BACKUP_EVERY, NIMRAG_BACKUP_KEEP
nimrag-server backup --data-dir DIR --out BACKUPS [--keep N]                  # one backup now
nimrag-server restore --from BACKUPS/nimrag-backup-<time> --data-dir NEWDIR
```

One backup is a folder `nimrag-backup-<UTC time>` (`-1`, `-2` if two start in the same second):

```
nimrag.db      snapshot taken with VACUUM INTO: consistent, and writers keep going while it runs
files/         the file store (blobs and sidecars), same layout as the data dir
backup.json    created_at, schema version, counts and sizes
```

* **Off unless `--backup-dir` is given.** `--backup-every` takes `30m`, `6h`, `1d` (a unit is
  required, at least `1m`). The first backup is due when the newest one in the folder is older than
  the interval (at once when there is none), so restarting the server does not add backups.
* **Incremental for files.** The database goes first, then the files, so every file the snapshot
  knows is in the copy. A file whose size and modification time match its copy in the previous
  backup is hard-linked from it; otherwise (or when linking fails, for instance on another
  filesystem) it is copied. A file inside a backup is never rewritten. Blobs never change;
  sidecars are replaced atomically, so a changed sidecar has a new modification time and is copied.
  A file modified in the last 100 ms is copied again next time (its time could still repeat).
* **All or nothing.** The backup is written to `.tmp-nimrag-backup-*` and renamed when complete. A
  failure (an unreadable file, a full disk) removes the temporary folder and keeps the old backups.
  Old backups are pruned only after a new one succeeded, never below one. Only folders named
  `nimrag-backup-<time>` are ever removed.
* **Logging.** One info line per backup (`backup done`: files, bytes, linked, copied, duration), a
  warning when one fails. A failed scheduled backup is tried again after `min(every, 1 h)`. The
  server keeps running either way.
* **The CLI** reads the database through a read-only connection and does not take the data-dir lock,
  so it works while `serve` runs (and runs beside a scheduled backup: one backup at a time per
  backup folder, guarded by `.backup.lock`). It refuses a backup folder inside the data dir.
  Without `--keep` it prunes nothing.
* **Restore** copies a backup folder into a new or empty data dir (a dir with anything in it is
  refused), checks the database with `PRAGMA quick_check`, and empties the target again if anything
  fails. It copies, it does not link, so the restored dir does not share files with the backup.
  Then start `serve` on it, or run `reparse` first. A file that arrived between the snapshot and the
  file copy has no database row; `rebuild-ledger` adds it (unowned).
* The database is copied whole, derived tables included (`fit_messages` is the bulk): a restore is
  then ready at once. Expect the backup to be about as big as `nimrag.db` plus the files.

Tests: `tests/backup.rs`. It runs backups while the router takes uploads, restores and compares every
table row by row, checks that the second backup hard-links (same inode) and copies only what
changed, pruning, a failing backup (no temporary folder, old backups kept), the scheduler (including
survival of a failure), and the real binary: `serve --backup-dir`, `backup` beside the running
server, `restore`, a server started on the restored dir that accepts the old token, and
restore then `reparse` giving the same derived tables as the original. The runbook has the same
check as commands (`docs/RUNBOOK.md`).

## Export

```
GET /api/export                                                  # signed-in user, plain download link
nimrag-server export --user USERNAME --out FILE.zip [--data-dir DIR]
```

A zip of one user's own data, the same from both entry points:

```
README.txt
csv/<kind>.csv            one per kind of data the user has; kinds with no rows are left out
csv/files.csv             every original: name in the zip, sha256, size, arrival, source
originals/<sha256>.fit    the user's original files, byte for byte (.fit by content, .json for an
                          imported Garmin Connect record, no extension for anything else)
```

CSV kinds: `activities`, `activity_sessions`, `activity_laps`, `activity_records` (with
positions), `activity_time_in_zone`, `strength_sets`, `exercise_titles`, `strength_set_edits`,
`sleep_sessions`, `sleep_stages`, `heart_rate`, `resting_hr`, `steps_activity`,
`intensity_minutes`, `daily_totals`, `connect_daily`, `stress_body_battery`, `respiration`, `spo2`,
`hrv_values`, `hrv_summary`, `training_status`, `training_load`, `training_balance`,
`recovery_time`, `vo2max`, `body_measurements`, `watch_battery`, `settings` (the list is `TABLES`
in `src/export.rs`; a new user-owned table gets an entry there).

* **Whose.** Sample tables by `user_id`; per-file tables (activities and what hangs on them, sleep
  sessions) through `activities_current` and `sleep_sessions_current`, which keep each user's best
  file per activity or night; originals through `file_owners`. Rows with `user_id` 0 and files
  nobody owns are in nobody's export. The `file` column of the activity CSVs is the original's
  name, so activities, sessions, laps, records and sets join on it.
* **Format.** UTF-8, header row, RFC 4180 quoting, `\n` line ends. Each time is two columns:
  ISO 8601 in the user's time zone and `<name>_epoch_s` (Unix seconds, UTC). A day (`daily_totals`,
  `connect_daily`) is `date` plus `date_start_epoch_s`. An empty cell is "no value", never 0. Units are in
  the column names. No internal ids beyond `file`, `session`, `lap`, `set` and `idx` (the position inside a file).
* **Access.** `GET /api/export` needs the session cookie of a user with data: an admin gets 403, a
  device token and no session get 401. No CSRF header (it is a `GET`, a link works), and the usual
  `Cache-Control: no-store`. At most two exports run at once server-wide; another gets 429 with
  `Retry-After`. The file name is `nimrag-export-<user>-<date>.zip`.
* **Streaming.** The zip is written to a non-seekable writer on a blocking thread as the rows and
  files are read, through a queue of 8 pieces of 64 KiB to the response body: a slow client slows
  the export, one that stops reading for two minutes ends it, and memory stays flat whatever the
  size. A failure part-way aborts the transfer instead of sending a short archive. Entries use
  zip64 sizes and data descriptors, deflate compression.
* **CLI.** `export` reads the database through a read-only connection and does not take the data-dir
  lock, so it works while `serve` runs; it writes `FILE.zip.part` and renames it when whole. It
  refuses an admin or unknown user, and a database a newer build has not migrated yet.
* A file the database lists but the store has lost is left out of `originals/`, logged, listed in
  `files.csv` with an empty name and counted in `README.txt`.

Tests: `tests/export.rs` (isolation between two users, the admin's 403, tokens and sessions, layout,
headers, time columns, quoting, activity joins, a lost file, every table of the seed against the
database row counts, the two-at-once limit, the CLI beside a running server) and
`tests/export_stream.rs` (48 MiB of incompressible files: resident memory grows by under 1 MiB, far
under the data size, while the client reads slowly and after).

## Docker

```
docker build -f server/Dockerfile -t nimrag-server .     # from the workspace root
docker run -v nimrag-data:/data -p 8080:8080 nimrag-server
docker run --rm -v nimrag-data:/data nimrag-server token create --name backup-script --data-dir /data
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

## Garmin Connect history

```
nimrag-server import-connect --user USERNAME [--data-dir DIR] [--timezone IANA] <dir>
```

Imports a year or so of the owner's Garmin Connect history (raw JSON pulled through a Connect
connector: sleep, HRV, training status and load, VO2 max, daily stats, activities) so the history
pages show data from before the watch was synced to this server. Reads files only (no network).
`--user` is required and names a normal user (not an admin); the data belongs to that user. The
command takes the data-dir lock, so it cannot run beside `serve` (stop the server, import, start
it again).

**Input.** Every `*.json` below `<dir>` is recognised by its shape, not its name:

| shape | kind | maps to |
| --- | --- | --- |
| `{"nights": [...]}` | `sleep` | `sleep_sessions` (start/end, score, stage totals), `resting_hr` |
| `{"trend": [...]}` with `last_night_avg_hrv_ms` / `weekly_avg_hrv_ms` | `hrv_trend` | `hrv_summary` |
| `{date: {..., baseline_balanced_low_ms, ...}}` | `hrv_daily` | `hrv_summary` with baselines |
| `{date: {training_status, acute_load, ...}}` | `training_status` | `training_status`, `training_load`, `training_balance` |
| `{"trend": [{atl, ctl, ...}]}` | `training_load` | `training_load`, `training_status` |
| `{"sport", "trend": [{vo2_max, ...}]}` | `vo2max` | `vo2max` (carried-forward days skipped) |
| `{"days": [{steps, total_calories, ...}]}` | `stats` | `daily_totals` (days in progress or without data skipped) |
| `[{date, steps, stress_avg, bb_high, resp_avg, moderate_min, ...}]` (a list of days, `daily_stats/YYYY-MM.json`) | `daily_stats` | `connect_daily` (see "Daily summaries" below) |
| `{stressValueDescriptorsDTOList, stressValuesArray, ...}` (a day, `intraday/stress/YYYY-MM-DD.json`) | `intraday_stress` | `stress` (stress level, body battery level), see "Intraday series" below |
| `{heartRateValueDescriptors, heartRateValues, ...}` (a day, `intraday/heart_rate/YYYY-MM-DD.json`) | `intraday_heart_rate` | `mon_hr` |
| `{respirationValueDescriptorsDTOList, respirationValuesArray, ...}` (a day, `intraday/respiration/YYYY-MM-DD.json`) | `intraday_respiration` | `respiration` |
| `{"activities": [...]}` (the list) | `activity_summary` | one blob per entry: `activities`, `activity_sessions` |
| `{"summary": {...}, "fit": {"messages": [...]}}` | `activity` | `activities`, `activity_sessions`, `activity_laps`, `strength_sets`, `exercise_titles`, `activity_metrics` |

Anything else is listed as "not Connect JSON" and not stored. Not imported (not in the data): HR
zone boundaries. Not imported from the intraday files: steps (the watch's cumulative counter covers
the whole day), the resting heart rate and the day's own minimum, maximum and averages (those
figures stay in the stored envelope), the body battery status words.

**Daily summaries (optional).** `<dir>/daily_stats/YYYY-MM.json`, a month per file, is a JSON array
of days (a folder that is absent is skipped silently; the importer finds the files by their shape
like the others):

```json
[{"date":"2026-11-12","steps":2327,"distance_m":1669,"floors_up":1.0,"floors_down":0.0,
  "moderate_min":0,"vigorous_min":0,"intensity_goal":350,"hr_min":58,"hr_max":101,"rhr":56,
  "stress_avg":28,"stress_max":76,"bb_charged":5,"bb_drained":33,"bb_high":65,"bb_low":34,
  "resp_avg":13.0,"resp_high":17.0,"resp_low":10.0}]
```

Any field may be null; dates are the owner's local days (in the envelope's zone). Each day is a
row of `connect_daily` (key: user, start of the local day; a sample table like `daily_totals`).
Steps, distance, floors, minutes and the body battery figures are amounts (0 is a value); heart
rate, stress and breathing are measurements, so 0 or a negative number (Garmin's "not measured")
is no value. A day with no figure at all is not a row; an entry without a valid date counts as
skipped (the blob's `file_derive` status is `partial`). `rhr`, `intensity_goal`, `floors_down` and
`stress_max` are kept in the stored envelope but no page uses them (resting heart rate has its
own Connect import; the API has no stress maximum), so a later migration plus `reparse` can use
them. Pull a month again later (a day more, a corrected number) and import again: the new file is
another blob that outranks the old one (later data, more rows), so every day it has replaces the
old row (no duplicates); the same file changes nothing. Pull complete days: a half day (today)
would stand as the day's figures until the watch has data for it.

**Intraday series (optional).** `<dir>/intraday/{stress,heart_rate,respiration}/YYYY-MM-DD.json`,
a day per file, is what the Connect connector returns for one day (found by shape like the others,
so the folder names do not matter; a folder that is absent is skipped). The columns are read by the
descriptor keys of each file, not by position, and the instants are epoch milliseconds, so a day
with a DST change (500 stress points) needs no special care. Every file becomes a blob of its own
(a day pulled again later with more samples is another blob that outranks the old one by its later
data; the same file changes nothing) and its rows go through the same derive path as a watch file,
so `reparse` rebuilds them. The account's `userProfilePK`, which these responses carry, is removed
before the envelope is stored (the rest of the response is kept as it came).

| file | rows | mapping |
| --- | --- | --- |
| `stress` | `stress`, one per instant with a value | `stressValuesArray` `[ms, level]` into `stress` (null and Garmin's -1 "no reading" are no value; -2 "too much motion" goes into `stress_raw`, as the watch's own activity reading does); `bodyBatteryValuesArray` `[ms, status, level, version]` into `body_battery` at the same instant (a null level is none). `body_battery_status` (a code, Connect sends a word) stay `NULL`. An instant with neither value is no row. |
| `heart_rate` | `mon_hr` | `heartRateValues` `[ms, bpm]` (null and 0 are no reading) |
| `respiration` | `respiration` | `respirationValuesArray` `[ms, brpm]` (-1.0 is no reading); `brpm_raw` stays `NULL` |

The rows are owned by the import's user and carry the Connect source's rank, which is below every
FIT source, so the sample tables' rank-based upsert decides per sample (the primary key is the
user and the instant): at an instant the watch also has a row for, the watch's row wins as a whole
(whatever the import order), and Connect's samples fill the rest of the day.

**Mixed days.** A day can have both: the first day of watch data (2026-11-19, watch rows only from
21:06) has Connect's from the start of the day and the watch's after. The Day, Today, Movement,
Calendar and metric pages compute every figure from the day's merged rows, never per source:
the stress average and the time at each level (a level stands for the time to the next reading,
at most 10 minutes), the energy series with its charged, drained, high, low and "now" (the latest
sample of either source), the heart rate low, high and 5-minute series, breathing (the awake and
asleep means by the night's start and end, the hourly chart) and the sleep window's lowest heart
rate and breathing. Notes: a stress reading of -2 (too much motion) counts as the activity level,
as the watch's negative raw reading does; a -1 is a gap (no stress value), whose time stands for
the reading before it, up to 10 minutes; where Connect and the
watch both cover the same period at different instants their rows interleave, so the means weigh
that period by both sample counts (the time per stress level does not, it follows the clock).
`system.last_data_ts` still only looks at FIT sources, so Connect's samples never make the data
look fresh.

**Fallback to the daily summaries.** The watch's FIT data always wins. A page reads Connect's
summary only for a day, and a metric, that has no sample row (the watch's, or intraday rows of
Connect's, which fill the same tables); if the day has any row of that metric, the numbers
computed from the rows are used as they are (a figure they lack stays `null`; the summary is not
mixed in), so a day with intraday series never shows the summary's numbers for those metrics and
a month pulled as summaries only still does. The helpers in `web_api/daily.rs` and `metrics.rs`
do it, so a day shows the same number on Today, the Day view, Movement, Calendar, the metric
ranges and Trends:

| metric | from `connect_daily` | row that makes the day count as measured (the watch's or Connect's intraday) |
| --- | --- | --- |
| stress | `stress_avg` as `stress.average` (no levels, no series, `main_level` null) | any `stress` row with a stress value or a raw status |
| energy (body battery) | `bb_high`, `bb_low`, `bb_charged`, `bb_drained` as `high`, `low`, `charged`, `drained` (no `now`, no series) | any `stress` row with `body_battery` |
| heart rate | `hr_min`, `hr_max` as the day's `low_bpm` and `high_bpm` (the rolling 24 hours stays a watch chart) | any `mon_hr` row |
| breathing | `resp_avg`, `resp_low`, `resp_high` as the day's awake average, lowest and highest (asleep stays `null`) | any `respiration` reading in the day or the night that ended on it |
| intensity minutes | `moderate_min`, `vigorous_min` (credited = moderate + 2 x vigorous) | any `mon_extra` running total that day |
| steps, distance | `steps` (after `daily_totals`), `distance_m` | the watch's `mon_activity` counters |
| floors | `floors_up` as the Movement page's `floors` | none (the watch has no table) |

Intensity minutes: the watch keeps a weekly running total, so its first reading after days that
came from Connect is the week so far; the day's increase is the total less what Connect counted
for the days before it in that week (never negative), so the week is not counted twice.

Where it shows: `/api/today` (`stress`, `energy`, `heart_rate`, `steps`, `intensity_minutes`,
`respiration.day_avg_brpm`, `yesterday`, `last_7_days`), `/api/days/{date}` (`stress`, `energy`,
`heart_rate.day`, `breathing`, `intensity_minutes`, `steps`), `/api/movement/{date}` (`floors`,
`distance_m`, `intensity`), `/api/calendar/{month}` (`energy_high`, `energy_low`, `steps`),
`/api/metrics/{stress,energy,heart_rate,respiration,intensity_minutes,steps}` (series, stats and
`has_prev`), `/api/trends` (`breathing.awake_avg_brpm`: with days the watch has no breathing
reading for, the mean of the days' awake means). A day that only has the summary (no intraday
series pulled for it) has no series and no hourly charts in the Day view; a day with intraday series
has them. Pull complete days: a day pulled while it is still going on has the samples until then,
and the summary of the same day is not read for the rest.

**Records and zones (optional).** `<dir>/records/<activity id>_<offset>.json` holds the pages of the
connector's `get_activity_fit_messages(include_records=true, message_types=["record",
"unknown_216", "zones_target"])` for an activity (several pages when it is long). The directory is
not walked as input; a missing directory or an activity without pages is imported as before.
For an activity file with pages the importer keeps, per `record` message with a timestamp and any
value, one `activity_records` row (`ts` unix seconds, `lat`/`lon` degrees, `hr`, `speed` m/s
(`enhanced_speed` preferred), `altitude_m` (`enhanced_altitude` preferred), `distance_m`, `cadence`,
`temperature_c`) and, per `time_in_zone` message (global 216, which the connector's decoder calls
`unknown_216`), one `activity_time_in_zone` row (`ref_mesg`, `ref_index`, seconds per zone, `null`
for an invalid element): the same units and the same rows a watch FIT file gives, so the activity
page's Route, Heart rate, Elevation and Time in heart rate zones fill. Fields are found by
definition number and, where the connector gave only `unknown_N`, scaled with the FIT profile's
factors (speed /1000, altitude /5 - 500, distance /100, zones /1000). Pages that overlap are
merged by message index. Not stored: zone boundaries (no column) and anything computed from the
heart rate. They go into the activity's envelope as a `records` member (compact rows, not the
raw pages), so they derive again on reparse and the same files give the same blob. An activity
imported earlier without them becomes a second blob with the same `connect:<id>` key that outranks
the first (more rows); the old blob stays stored and `activities_current` hides it, so run the
import again after pulling the records. A page for another activity (`activity_id` mismatch) is
reported as an error and the activity is imported without records; pages for an id with no activity
file are counted and ignored.

**Storage.** Each input becomes a content-addressed blob (`files/<aa>/<bb>/<sha256>`, ledgered
with `source = connect`) holding an envelope around the original JSON, byte for byte:
`{"nimrag_connect":1,"kind":"sleep","tz":"Europe/Copenhagen","data":<the original>}`. The blob is
recognised from its first bytes (like FIT), so `ingest`, `reparse` and an admin's assign handle it
with no other state: `reparse` derives it again, deleting a user leaves the blobs unassigned, and
assigning them to a user derives their rows again for the new owner. The envelope carries the IANA
time zone used for Connect's local dates and times (`--timezone` if given, else the user's
`timezone` setting); the same JSON imported under another zone is another blob. Re-importing is
idempotent: identical input gives identical blobs, which are already stored and ledgered (the
path inside `<dir>`, plus `#<id>` for a list entry, is the `origin` and folds repeats into the
existing event); nothing is derived twice.

**Time.** Sleep start and end are UTC instants (epoch milliseconds); decoded FIT messages carry UTC
ISO strings; an activity summary has both `start_time_gmt` and `start_time_local`; the activity
list has a local `start_time`; everything else is per local date. Local times and dates are
resolved in the envelope's zone (the first reading of an hour that happens twice). A row Connect
gives only per date sits at the start of that local day, so a watch reading later the same day is
the day's value in the range endpoints.

**Rank: the watch wins.** A Connect source has `sources.origin = 'connect'` and a rank key
starting with `-` (`-:<kind priority>:<last_ts>:<rows>`), which sorts below every FIT key (those
start with `0` or `1`). Sample rows (HRV, training status and load, VO2 max, resting heart rate,
`daily_totals`, `connect_daily`) use the usual rank-based upsert, so a FIT row for the same key replaces a Connect
row and a Connect row never replaces a FIT row, whichever is imported first; a reparse gives the
same result. Between two Connect sources the richer kind wins (the dedicated training status over
the load history, the daily HRV with baselines over the trend, a detailed activity over its list
entry), then the later data, then more rows.

**Dedup of activities and nights.** The views `activities_current` and `sleep_sessions_current`
hide a Connect activity when a FIT activity of the same user (from the watch or from
`import-export`) starts within 120 seconds of it, and a Connect night when a FIT night of the same
user overlaps it in time. Hidden, not deleted: the rows stay derived, and the Connect item shows
again if the FIT file is removed. Another user's FIT files never hide anything.

**Pages.** `sleep_sessions` has stage totals (`asleep_s`, `deep_s`, `light_s`, `rem_s`,
`awake_s`) for a Connect night; time asleep is deep + light + REM (what the watch shows), not the
time in bed. A watch night keeps using its `sleep_levels` rows. `daily_totals` (steps, total,
active and resting calories per local day) fills the Today and Movement pages for a day without
minute data. `system.last_data_ts` and the Garmin Forward backfill only look at FIT sources.

Migration `0012_connect_daily.sql`: the `connect_daily` table. Migration `0011_connect.sql`: `sources.origin`, the five sleep columns, `daily_totals`, the two
views, and a rebuild of `files` so its `source` CHECK accepts `connect` (SQLite cannot alter a
CHECK; the rows go through a temp table with the foreign keys deferred to commit, nothing is lost,
tested in `tests/migration_connect.rs`).

Tests for the intraday series: `tests/connect_intraday.rs` with the fixture
`tests/fixtures/connect_intraday/` (made-up numbers; the DST day is generated in the test): the
three shapes found anywhere below the directory, the import, its re-run and `reparse`, a day pulled
again, `userProfilePK` stored nowhere, a Connect-only day (series, stress levels, energy, heart rate,
breathing, Today, the metric pages), the sleep window, a mixed day with a watch sample at the same
instant as a Connect one (watch wins, in either import order) and the day's figures from the
merged rows, the summary used only for a day (and metric) without samples, the 25-hour DST day,
and another user seeing nothing; unit tests for the shapes, the descriptor keys and the
-1, -2 and null values in `connect/` and `connect/import.rs`.

Tests for the daily summaries: `tests/connect_daily.rs` with the fixture
`tests/fixtures/connect_daily/daily_stats/` (made-up numbers): the import and its re-run, a later
pull of the month, a directory without the folder, the Day, Today, Movement, Calendar, metric
and Trends pages filling from it, a day (and a metric) with watch data keeping the watch's
numbers, the intensity week built from Connect days and a watch total, and another user seeing
nothing.

Tests (`tests/connect.rs`, synthetic JSON with made-up numbers; real data never goes in the
repository): the mapping and the pages, records and zones (named and `unknown_N` fields, overlapping
pages, invalid values, the activity page's series, track and zones, records added by a later
re-run, a watch activity still hiding a Connect one with records), an idempotent re-import, the user's zone, the CLI's
refusals, reparse gives the same rows, delete-then-assign moves the history, another user sees
nothing, and (with the FIT profile) a watch row or activity wins in both import orders.

`serve`, `import-export`, `import-connect`, `reparse`, `rebuild-ledger` and `dev-seed` take an
exclusive lock on `<data>/.lock`; a second one fails fast with "data dir ... in use by `<command>`".
(`token` and `user` do not lock.) This needs Rust 1.89 for `File::try_lock`, so the server
crate sets `rust-version = "1.89"`; core stays at 1.80.
