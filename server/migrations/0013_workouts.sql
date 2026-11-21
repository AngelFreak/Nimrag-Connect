-- Workouts built on the web and sent to the watch (stage 4a, 2026-11-21).
--
-- One row per workout of a user. `definition` is the JSON the API takes and gives (the
-- `WorkoutBody`: name, sport, note, nodes); the FIT file is made from it when the phone asks, so
-- nothing derived is stored and a change of the encoder needs no migration. `revision` counts
-- edits: the phone reports which revision it sent, so a result for a workout that was edited in
-- the meantime does not mark the new version as sent.
--
-- `send_state`: idle (never sent, or edited since), queued (the owner pressed "Send to watch";
-- the phone fetches it on its next connection), sent (the phone handed the file to the watch and
-- the watch took it), failed (`send_error` says why). Owner data, not ledger data: `reparse`
-- never touches it.
CREATE TABLE workouts (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id           INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    sport             TEXT NOT NULL,
    definition        TEXT NOT NULL,
    revision          INTEGER NOT NULL DEFAULT 1,
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL,
    send_state        TEXT NOT NULL DEFAULT 'idle'
                      CHECK (send_state IN ('idle', 'queued', 'sent', 'failed')),
    send_requested_at INTEGER,
    send_finished_at  INTEGER,
    send_error        TEXT
) STRICT;
CREATE INDEX workouts_user ON workouts (user_id, updated_at DESC);
