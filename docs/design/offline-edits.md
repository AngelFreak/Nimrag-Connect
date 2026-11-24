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

1. The phone's copy accepts the requests in the table above. It runs the normal handler on its own
   database (so the page shows the change at once), then appends an outbox row in the same
   transaction: op id (random, unique), method, path, body, made-at time, and for a create the new
   local id.
2. The engine drains the outbox after the file uploads of each pass (same worker, same network rules,
   same backoff): oldest first, one at a time, `POST /api/device/edits` with
   `{op_id, method, path, body, made_at}`.
3. The server checks the request is on the list above, runs it through the same handler as the token's
   user, and remembers the op id (`device_edits_applied`), so a replay after a lost answer does
   nothing twice. It answers with the handler's status and body.
4. Ids: a path names a row by the phone's id. Before sending, the phone rewrites it to the server's id:
   - rows created on the phone (workouts, body entries): from an id map filled from each create's
     answer (`local id → server id`);
   - activities: by a key both sides share (the activity's source file hash, plus its start time),
     never by row id.
5. Outcome per op: 2xx → done (record the id map for a create). 404 or another 4xx → the change can't
   apply (e.g. the workout was deleted on the web meanwhile): drop it, log one line, and show it once
   ("1 change made offline could not be saved: …"). 401/403 → stop, like file uploads. IO, 5xx, 408, 429
   → back off and retry.
6. When the outbox is empty, pull the state (above): the copy then carries the server's ids again.

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
