# Offline edits: the phone's copy accepts changes and sends them on (design, 2026-11-24)

The owner's rule: "If the user changes data on the phone app, that should be pushed to the server
as well, when available." Offline must be transparent: the same pages, the same buttons, the change
shows at once, and it reaches the server without the person doing anything.

Today (0.8.2) the phone's copy (`server/src/embedded.rs`, stage 3o) is read-only over HTTP and holds
only what the phone itself got from the watch. Both change here.

## What can be changed offline

The person's own data, through the same API the web app already uses:

| what | requests |
|---|---|
| settings (goals, timezone, Home cards, …) | `PATCH /api/settings` |
| weight and body entries | `POST /api/body`, `DELETE /api/body/{id}` |
| strength session edits | `PATCH /api/activities/{id}/strength` |
| workouts | `POST /api/workouts`, `PUT/DELETE /api/workouts/{id}`, `POST/DELETE /api/workouts/{id}/send` |

Everything else stays online-only and keeps a clear 403 message ("needs the server: … "): sign-in,
password, two-step login, phones/tokens, admin, Garmin account and Send to Garmin settings, imports.
These are security or server-side actions that make no sense on a copy.

## Down: the server's state onto the phone

New `GET /api/device/state` (phone token auth, like the other `/api/device/*` routes): the token
user's settings, workouts (full definitions and send state), body entries and strength edits, as one
JSON document. The phone applies it to its copy (`Embedded::apply_state`), replacing those rows for the
local user. Pulled after an upload pass, on app start and on resume, **only while the outbox is
empty** (otherwise a pull would undo edits not yet sent).

## Up: the outbox

1. The phone's copy accepts the requests in the table above (`server/src/edits.rs`, `Edit::parse`,
   shared with the server). It runs the normal handler on its own database, so the page shows the
   change at once, and writes an outbox row (`edit_outbox`): op id (random, 32 hex), method, path,
   body, made-at time, and for a create the new local id. The handlers write straight to the
   database pool, so one transaction cannot span handler and row. Instead the row is written
   first (`ready` = 0) and confirmed (`ready` = 1, local id filled in) when the handler succeeded,
   or deleted when it failed. "Handler succeeded" therefore implies "row exists", also through a
   crash; a row whose handler was cut off by a crash is confirmed on the next start and the server
   decides (it runs the same handler and may answer 4xx). Changes are made one at a time (a mutex),
   so the outbox order is the order of the changes, and in a task of their own, so a client that
   hangs up cannot split them. Every other write keeps a 403 ("this needs the server: ...").
2. The engine drains the outbox after the file uploads of each pass (same worker, same network rules,
   same backoff): oldest first, one at a time, `POST /api/device/edits` with
   `{op_id, method, path, body, made_at}`. `body` is the request body as JSON *text* (a string, empty
   for none), `made_at` unix seconds.
3. The server checks the request is on the list above (else 400), runs it through the same router as
   the web app, as the token's user, and remembers the op id (`device_edits_applied`, per user), so a
   replay after a lost answer gets the first answer and changes nothing. It answers with the
   handler's status and body. Answers meaning "try later" (5xx, 408, 429) are not remembered, so a
   retry runs again. A `POST /api/body` without `measured_at` gets `made_at` as its time, so a weight
   logged offline is dated when it was logged, not when it arrived.
4. Ids: a path names a row by the phone's id. When the engine asks for the next change the phone
   rewrites it to the server's id:
   - rows created on the phone (workouts, body entries): from an id map (`edit_id_map`, `local id ->
     server id`) filled from each create's 2xx answer (its `id`). A create the server refused maps to
     nothing: later changes of that row are dropped (and reported) without being sent;
   - activities: by a key both sides share. **Wire form: `/api/activities/{sha256}@{start}/strength`**,
     where `{sha256}` is the activity's id (the hash of its watch file, the same on the phone and on
     the server because the server got the file from the phone) and `{start}` is the activity's
     start time in unix seconds (`activities.start_ts`). The server uses the hash when this user has
     that file; else the one current activity of this user that started at `{start}`; else 404 (also
     for two). The ordinary web route only takes the plain hash. The strength edits themselves are
     stored under the activity's `activity_key`, which both databases derive from the file, so they
     agree regardless of the id.
5. Outcome per op: 2xx → done (record the id map for a create). 404 or another 4xx → the change can't
   apply (e.g. the workout was deleted on the web meanwhile): drop it, keep one line for
   `take_dropped_edits` and show it once ("1 change made offline could not be saved: ..."). 401/403 →
   stop, like file uploads. IO, 5xx, 408, 429 → back off and retry. (Exact table under "FFI".)
6. When the outbox is empty, pull the state (above): the copy then carries the server's ids again and
   the id map is cleared. `apply_state` refuses (returns false) while any change is in the outbox, so a
   change made during the pull cannot be undone.

"Send to watch" offline: the phone's copy only queues it (`send_state = queued`) without writing
the FIT file (the phone's build may have no profile; `AppState::with_deferred_watch_send`), and
reports `watch_send.available = true`. The server writes the file when the real send reaches it
and answers 503 if it cannot; that is retried like any 5xx.

## Server endpoints

`POST /api/device/edits` and `GET /api/device/state`, phone token (`Authorization: Bearer`), no CSRF
header, a token without a user gets 403. `GET /api/device/state` answers
`{version: 1, settings: {key: text}, workouts: [row], body: [row], strength: [row]}` with the stored
rows (ids included): `user_settings` for `timezone`, `step_goal`, `intensity_goal_min`, `home_cards`;
`workouts` with `definition` as JSON text and the send state columns; `body_measurements`;
`strength_set_overrides` (`activity_ident`, `set_idx`, `fields` as JSON text, `updated_at`).

## FFI

On `LocalServer` (`ffi/src/local_server.rs`, feature `local-server`). All blocking, call them off the
main thread; each fails with `LocalServerError`.

| call | meaning |
|---|---|
| `next_edit() -> PendingEdit?` | the oldest change, or null. `PendingEdit {op_id, method, path, body, made_at}`, path already rewritten. Does not remove it: asking again returns the same one. |
| `edit_done(op_id, status: u16, response_body) -> EditVerdict` | report the server's answer; `status` 0 if there was none. `Applied` (2xx; gone, a create's id remembered), `Dropped` (4xx except 401, 403, 408, 429; gone, listed by `take_dropped_edits`), `Kept` (everything else; stays, back off). Reporting an op twice is fine (`Applied`). |
| `pending_edits() -> u32` | changes waiting; the banner count. |
| `take_dropped_edits() -> [DroppedEdit {method, path, reason}]` | the changes that could not be applied, once (forgotten as returned). `path` is the phone's own path. |
| `apply_state(json) -> bool` | the body of `GET /api/device/state`; false and nothing changed while changes wait; an error for anything that is not the state. |

The drain loop, per pass, after the file uploads:

```
while let e = next_edit():
    (status, body) = POST {server}/api/device/edits  Bearer token,  json {op_id, method, path, body, made_at}
    match edit_done(e.op_id, status, body):
        Applied | Dropped -> continue
        Kept -> stop the pass, back off          // also on 401/403
if pending_edits() == 0:
    json = GET {server}/api/device/state; apply_state(json)   // false: a change arrived meanwhile, next pass
```

## Conflicts

Last change wins, in the order the changes were made. Settings are patches, so a goal changed on
the phone does not undo a timezone changed on the web. A workout edited on both: the later replay
wins. Nothing is merged inside one workout.

## The page

The banner keeps saying it is the phone's copy, without "(read-only)", and adds "N changes waiting"
while the outbox is not empty. The Watch page shows the same count next to waiting files.

## Not in this step

- Garmin history imported on the server (Connect export, daily summaries, intraday) is not copied to
  the phone; only the watch data the phone holds itself plus the state above.
- A workout "sent to the watch" offline is sent when the server is back (the phone fetches it from the
  server as today), not straight from the phone.
- Offline edits in a desktop browser (no phone copy there).

## Tests that must exist

- Server: each allowed request through `/api/device/edits` (incl. a replay of the same op id), a
  refused online-only path, 404 for a deleted row, token auth, user isolation.
- Embedded: an allowed write changes the copy and adds one outbox row; an online-only write still 403s;
  `apply_state` replaces the rows; the id map rewrites a later edit of a workout created offline;
  an activity path is rewritten by its key.
- Android: drain order, outcomes per status, pull only with an empty outbox, the banner count.
- End to end on the Samsung against a local server: offline create a workout and change a goal → both
  show on the phone → server back → both on the server and in the web; then edit the same workout on
  the web and pull.
