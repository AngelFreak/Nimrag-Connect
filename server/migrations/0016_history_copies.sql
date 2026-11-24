-- History on the phone (2026-11-24, docs/design/history-on-phone.md).
--
-- Phone side (the embedded copy; unused on a server). `history_copies` marks the files the
-- phone copied from the real server, as opposed to the ones its own watch delivered, so a
-- smaller "Server history on this phone" choice can remove exactly those. `last_ts` is the
-- server's `sources.last_ts` for the file (the window is by data date); NULL when unknown, and
-- such a file is never pruned by a window.
CREATE TABLE history_copies (
    sha256    TEXT PRIMARY KEY CHECK (length(sha256) = 64),
    last_ts   INTEGER,
    copied_at INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
