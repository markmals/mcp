-- Notes, keyed by their name. `revision` starts at 1 and increases on every change, so a
-- writer can refuse to overwrite a version it has not seen (`--expect-revision`).
CREATE TABLE notes (
    name TEXT NOT NULL PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
