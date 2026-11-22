# The store

mcp keeps notes in one SQLite file, the store. Every command that reads or writes notes uses exactly one store, chosen the same way each time.

## Choosing the store

1. `--store <path>` on the command, relative to the current directory.
2. Otherwise the `MCP_STORE` environment variable. An empty value counts as unset.
3. Otherwise `$XDG_DATA_HOME/mcp/store.sqlite3` when `XDG_DATA_HOME` is an absolute path, else `~/.local/share/mcp/store.sqlite3`, or `%LOCALAPPDATA%\mcp\store.sqlite3` on Windows.

`mcp context show` prints the store in effect: its absolute `path`, the `source` of that path (`flag`, `environment` or `default`), and its `state`. Run it before changing anything, and pass the same `--store` to every command of a task. Use a temporary store to experiment:

```sh
export MCP_STORE="$(mktemp -d)/store.sqlite3"
mcp context show
```

## States

`context show` reports one of four states:

- `absent`: no store yet. Reads return nothing, `notes update` and `notes delete` fail with `NOT_FOUND`, and `mcp notes create` or `mcp store migrate` creates the file, its directory and its schema.
- `ready`: the store has this mcp's schema; `notes` counts its notes.
- `needs-migration`: the store was written by an older mcp. Other commands fail with `PRECONDITION_FAILED` until `mcp store migrate` upgrades it; `pendingMigrations` lists what it would apply.
- `unusable`: the path is a directory, not a SQLite file, a store written by a newer mcp, or a SQLite database another program wrote (it has tables but no mcp migration history); `problem` says which. Every command, `store migrate` included, refuses it with `PRECONDITION_FAILED` and leaves it unchanged.

## Migrations

The schema is versioned by migrations compiled into the binary. After upgrading mcp, run `mcp store migrate --dry-run` to see the migrations a store needs, then `mcp store migrate` to apply them. Migrating a current store applies nothing, so it is safe to repeat. A store written by a newer mcp is refused rather than changed; use that mcp, or a different store.

## Reads never write

Reads and dry runs open an existing store read-only and never create one, so `context show`, `notes list`, `notes get` and any `--dry-run` leave the file system exactly as they found it. Only a modifying command run without `--dry-run` writes.

## Concurrency

Each change reads the note and writes it inside one write transaction, so two processes cannot interleave a read and a write. A command waits up to five seconds for another process's write lock, then fails with `UNAVAILABLE`, exit status 5 and `retryable` true; running the same command again can succeed. To refuse a note another process changed since you read it, pass `--expect-revision`.

## Backups

The store is an ordinary SQLite file. Copy it while no command is writing to back it up, or use `sqlite3 <path> ".backup <copy>"` while it is in use.
