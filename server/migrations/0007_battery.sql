-- Device battery level (FIT `battery`, message 104): sample table, natural key ts.
CREATE TABLE battery (
    ts      INTEGER PRIMARY KEY,
    percent REAL NOT NULL,
    src     INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
