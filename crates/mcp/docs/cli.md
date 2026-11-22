# CLI reference

`mcp` is an agent-first command-line tool for keeping notes in a local SQLite store. This page is generated from the command registry by `mise run cli:reference`; do not edit it by hand.

Every command follows one contract:

- **Results** are JSON on stdout, whether or not stdout is a terminal: compact when piped, indented on a terminal. `--output text` renders the same result for people. Warnings and errors go to stderr.
- **Collections** return `items`, `hasMore`, and `nextCursor`; pass `nextCursor` back as `--cursor` until `hasMore` is false.
- **Modifying commands** accept `--dry-run`, which validates the input, resolves the store, and returns the receipt the command would produce without writing anything, not even a new store. Receipts carry a `status` (`completed` or `dry-run`), the `store`, and the resulting resource.
- **Failures** write one JSON document to stderr and exit with the code's status (see below). Nothing is written to stdout.
- **No command prompts.** Missing input fails at once with `INVALID_ARGUMENT`.
- **The store** is `--store`, else `MCP_STORE`, else `$XDG_DATA_HOME/mcp/store.sqlite3` (`~/.local/share/mcp/store.sqlite3`, or `%LOCALAPPDATA%\mcp\store.sqlite3` on Windows). `mcp context show` prints the one in effect.

Discover a command with `mcp cli search "<task>"`, inspect it with `mcp schema <resource> <operation>`, preview it with `--dry-run`, run it, and verify the result with a read such as `mcp notes get <name>`.

## Options every command accepts

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--output <choice>` | `json`, `text` | no | `json` | Result format: JSON (compact when piped, indented on a terminal) or text for humans |

## Errors

| Code | Exit | Retryable | Meaning |
| --- | --- | --- | --- |
| `INVALID_ARGUMENT` | 2 | no | Input is missing or malformed; `details` names the argument and what it accepts. |
| `NOT_FOUND` | 3 | no | A named note, command, or document does not exist. |
| `PRECONDITION_FAILED` | 3 | no | The store is not ready: it needs `mcp store migrate`, was written by a newer mcp, or is not a mcp store. |
| `CONFLICT` | 4 | no | The operation conflicts with the store's current state: the name is taken, or the note's revision is not the one passed to --expect-revision. |
| `UNAVAILABLE` | 5 | yes | Another process holds the store's write lock; retry the same command. |
| `INTERNAL` | 70 | no | A defect in mcp itself, or a stdout that cannot take the result; `message` says which. |

Every failure writes this document to stderr:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "ErrorDocument",
  "description": "What a failing command writes to stderr, as one JSON document.",
  "type": "object",
  "properties": {
    "error": {
      "$ref": "#/$defs/ErrorBody"
    }
  },
  "required": [
    "error"
  ],
  "$defs": {
    "ErrorBody": {
      "type": "object",
      "properties": {
        "code": {
          "description": "Stable code; branch on this, not on the message.",
          "$ref": "#/$defs/ErrorCode"
        },
        "message": {
          "description": "One sentence for a person.",
          "type": "string"
        },
        "retryable": {
          "description": "Whether running the same command again, unchanged, can succeed.",
          "type": "boolean"
        },
        "details": {
          "description": "Facts about the failure, such as the argument, the value, or the conflicting revision."
        },
        "suggestions": {
          "description": "Commands or changes that recover from the failure, most useful first.",
          "type": "array",
          "items": {
            "type": "string"
          }
        }
      },
      "required": [
        "code",
        "message",
        "retryable",
        "details",
        "suggestions"
      ]
    },
    "ErrorCode": {
      "description": "A stable error code. Codes, exit statuses, and retryability are part of the CLI contract.",
      "oneOf": [
        {
          "description": "Input is missing or malformed.",
          "type": "string",
          "const": "INVALID_ARGUMENT"
        },
        {
          "description": "A named note, command, or document does not exist.",
          "type": "string",
          "const": "NOT_FOUND"
        },
        {
          "description": "The store is not ready for the operation.",
          "type": "string",
          "const": "PRECONDITION_FAILED"
        },
        {
          "description": "The operation conflicts with the store's current state.",
          "type": "string",
          "const": "CONFLICT"
        },
        {
          "description": "The store is busy; retrying can succeed.",
          "type": "string",
          "const": "UNAVAILABLE"
        },
        {
          "description": "A defect in `mcp` itself, or a stdout that cannot take the result.",
          "type": "string",
          "const": "INTERNAL"
        }
      ]
    }
  }
}
```

## Commands

### `cli`

Find the command for a task.

#### `mcp cli search`

Find the mcp command for a task described in plain words.

Ranks every command against the query by its name, summary, description, arguments, and declared synonyms, and returns the best matches with the exact command to run and whether it reads or modifies state. Runs locally and never changes anything. Inspect a match with `mcp schema <command>` before running it.

- **Effect:** reads only
- **Requires:** nothing beyond `mcp`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<query>` | string, repeatable | yes | — | What you want to do, in plain words; several words need no quotes |
| `--limit <integer>` | integer, 1–20 | no | `5` | The most matches to return |
| `--cursor <string>` | string | no | — | Resume after a previous page: the nextCursor it returned |

Examples:

```sh
# Find how to add a note
mcp cli search 'add a note'
# Find how to change a note, three matches at most
mcp cli search 'edit the title of a note' --limit 3
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/CommandMatch"
      }
    },
    "hasMore": {
      "type": "boolean",
      "description": "Whether more items follow this page."
    },
    "nextCursor": {
      "type": [
        "string",
        "null"
      ],
      "description": "Pass as `--cursor` to get the next page; null when there is none."
    }
  },
  "required": [
    "items",
    "hasMore"
  ],
  "description": "One page of a collection.",
  "title": "Page",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "CommandMatch": {
      "type": "object",
      "properties": {
        "command": {
          "type": "string",
          "description": "The exact command to run, without its arguments."
        },
        "id": {
          "type": "string",
          "description": "The registry identifier, such as `notes.create`."
        },
        "summary": {
          "type": "string"
        },
        "effect": {
          "type": "string",
          "description": "`reads`, or `modifies <what>`."
        },
        "score": {
          "type": "number",
          "format": "double",
          "description": "Relevance from 0 to 1."
        }
      },
      "required": [
        "command",
        "id",
        "summary",
        "effect",
        "score"
      ],
      "description": "One command that fits the query."
    }
  }
}
```

### `schema`

Inspect a command's full specification.

#### `mcp schema`

Show a command's arguments, defaults, effects, errors, examples, and output schema.

With no command, lists every command with its summary and effect. With a command path such as `notes create` (or its identifier, `notes.create`), returns that command's full registry entry: each argument with its type, whether it is required, its default, and its accepted values or bounds; whether it reads or modifies state and what it requires; whether it supports --dry-run and is idempotent; the error codes it can return with their exit statuses; examples; and the JSON Schema of its result. Never changes anything.

- **Effect:** reads only
- **Requires:** nothing beyond `mcp`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `NOT_FOUND`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<command>` | string, repeatable | no | — | The command path, such as `notes create`; omit to list every command |

Examples:

```sh
# List every command
mcp schema
# Inspect note creation
mcp schema notes create
```

Result schema:

```json
{
  "anyOf": [
    {
      "$ref": "#/$defs/CommandList",
      "description": "With no command: every command."
    },
    {
      "$ref": "#/$defs/CommandDescription",
      "description": "With a command: its full specification."
    }
  ],
  "title": "SchemaResult",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "CommandList": {
      "type": "object",
      "properties": {
        "items": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/CommandSummary"
          }
        }
      },
      "required": [
        "items"
      ]
    },
    "CommandSummary": {
      "type": "object",
      "properties": {
        "command": {
          "type": "string"
        },
        "id": {
          "type": "string"
        },
        "summary": {
          "type": "string"
        },
        "effect": {
          "type": "string",
          "description": "`reads`, or `modifies <what>`."
        }
      },
      "required": [
        "command",
        "id",
        "summary",
        "effect"
      ]
    },
    "CommandDescription": {
      "type": "object",
      "properties": {
        "id": {
          "type": "string",
          "description": "Stable identifier: the command path joined with dots."
        },
        "command": {
          "type": "string",
          "description": "The command to run, without arguments."
        },
        "summary": {
          "type": "string"
        },
        "description": {
          "type": "string"
        },
        "arguments": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/ArgumentDescription"
          },
          "description": "The command's own arguments, then the options every command accepts."
        },
        "effect": {
          "$ref": "#/$defs/EffectDescription"
        },
        "requires": {
          "type": "array",
          "items": {
            "type": "string"
          },
          "description": "What the command needs besides mcp: `store` is the note store `mcp context show`\nreports."
        },
        "dryRun": {
          "type": "boolean",
          "description": "Whether the command accepts `--dry-run`."
        },
        "idempotent": {
          "type": "boolean",
          "description": "Whether running it twice with the same input leaves the same state as running it once."
        },
        "errors": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/ErrorDescription"
          },
          "description": "Every error the command can report on stderr."
        },
        "examples": {
          "type": "array",
          "items": {
            "$ref": "#/$defs/ExampleDescription"
          }
        },
        "output": {
          "description": "JSON Schema of the result the command writes to stdout."
        }
      },
      "required": [
        "id",
        "command",
        "summary",
        "description",
        "arguments",
        "effect",
        "requires",
        "dryRun",
        "idempotent",
        "errors",
        "examples",
        "output"
      ],
      "description": "A command's full specification, as `mcp schema <command>` returns it."
    },
    "ArgumentDescription": {
      "type": "object",
      "properties": {
        "name": {
          "type": "string"
        },
        "kind": {
          "type": "string",
          "description": "`positional`, `option`, or `flag`."
        },
        "usage": {
          "type": "string",
          "description": "How to write it: `<name>`, `--limit <integer>`, `--dry-run`."
        },
        "type": {
          "type": "string",
          "description": "`string`, `path`, `integer`, `name` (matches `pattern`), `text` (text, `@<file>`, or `-`\nfor stdin), `choice`, or `boolean`."
        },
        "required": {
          "type": "boolean"
        },
        "default": {
          "type": [
            "string",
            "null"
          ]
        },
        "multiple": {
          "type": "boolean",
          "description": "Whether it accepts several values."
        },
        "acceptedValues": {
          "type": [
            "array",
            "null"
          ],
          "items": {
            "type": "string"
          },
          "description": "The only values a `choice` accepts."
        },
        "minimum": {
          "type": [
            "integer",
            "null"
          ],
          "format": "uint64",
          "minimum": 0,
          "description": "The smallest `integer` accepted."
        },
        "maximum": {
          "type": [
            "integer",
            "null"
          ],
          "format": "uint64",
          "minimum": 0,
          "description": "The largest `integer` accepted."
        },
        "pattern": {
          "type": [
            "string",
            "null"
          ],
          "description": "The regular expression a `name` matches."
        },
        "maxLength": {
          "type": [
            "integer",
            "null"
          ],
          "format": "uint",
          "minimum": 0,
          "description": "The most characters a `name` may have."
        },
        "summary": {
          "type": "string"
        }
      },
      "required": [
        "name",
        "kind",
        "usage",
        "type",
        "required",
        "multiple",
        "summary"
      ]
    },
    "EffectDescription": {
      "type": "object",
      "properties": {
        "kind": {
          "type": "string",
          "description": "`reads` or `modifies`."
        },
        "what": {
          "type": [
            "string",
            "null"
          ],
          "description": "What a modifying command changes."
        }
      },
      "required": [
        "kind"
      ]
    },
    "ErrorDescription": {
      "type": "object",
      "properties": {
        "code": {
          "type": "string"
        },
        "exit": {
          "type": "integer",
          "format": "uint8",
          "minimum": 0,
          "maximum": 255,
          "description": "The exit status the error sets."
        },
        "retryable": {
          "type": "boolean",
          "description": "Whether retrying the same command can succeed."
        },
        "meaning": {
          "type": "string"
        }
      },
      "required": [
        "code",
        "exit",
        "retryable",
        "meaning"
      ]
    },
    "ExampleDescription": {
      "type": "object",
      "properties": {
        "summary": {
          "type": "string"
        },
        "command": {
          "type": "string"
        }
      },
      "required": [
        "summary",
        "command"
      ]
    }
  }
}
```

### `docs`

Search and read mcp's documentation.

#### `mcp docs search`

Search mcp's documentation: concepts, the store, and the CLI reference.

Ranks every section of the Markdown compiled into this binary against the query and returns the best matches: the document `path`, its `title`, the `section` heading, an `excerpt` of the best-matching paragraph, and a `score`. Read a whole document with `mcp docs show <path>`. Runs offline and never changes anything.

- **Effect:** reads only
- **Requires:** nothing beyond `mcp`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<query>` | string, repeatable | yes | — | The topic, in plain words; several words need no quotes |
| `--limit <integer>` | integer, 1–20 | no | `5` | The most matches to return |
| `--cursor <string>` | string | no | — | Resume after a previous page: the nextCursor it returned |

Examples:

```sh
# Learn how list pages continue
mcp docs search 'pagination cursor'
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/SectionMatch"
      }
    },
    "hasMore": {
      "type": "boolean",
      "description": "Whether more items follow this page."
    },
    "nextCursor": {
      "type": [
        "string",
        "null"
      ],
      "description": "Pass as `--cursor` to get the next page; null when there is none."
    }
  },
  "required": [
    "items",
    "hasMore"
  ],
  "description": "One page of a collection.",
  "title": "Page",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "SectionMatch": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string",
          "description": "The document, for `mcp docs show <path>`."
        },
        "title": {
          "type": "string",
          "description": "The document's title."
        },
        "section": {
          "type": "string",
          "description": "The heading of the matching section."
        },
        "excerpt": {
          "type": "string",
          "description": "The paragraph of the section that best matches the query, shortened to 400 characters."
        },
        "score": {
          "type": "number",
          "format": "double",
          "description": "Relevance from 0 to 1."
        }
      },
      "required": [
        "path",
        "title",
        "section",
        "excerpt",
        "score"
      ],
      "description": "One section that matches the query."
    }
  }
}
```

#### `mcp docs show`

Print one documentation file in full.

Returns the Markdown of one document compiled into this binary, by the `path` that `mcp docs search` reports, such as `docs/notes.md`. `--output text` prints the Markdown alone. Never changes anything.

- **Effect:** reads only
- **Requires:** nothing beyond `mcp`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `NOT_FOUND`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<path>` | string | yes | — | The document, as `docs search` names it: `docs/notes.md` |

Examples:

```sh
# Read the notes concepts
mcp docs show docs/notes.md
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string"
    },
    "title": {
      "type": "string"
    },
    "markdown": {
      "type": "string",
      "description": "The document's Markdown."
    }
  },
  "required": [
    "path",
    "title",
    "markdown"
  ],
  "description": "A whole document.",
  "title": "DocumentContent",
  "$schema": "https://json-schema.org/draft/2020-12/schema"
}
```

### `context`

Show where commands take effect.

#### `mcp context show`

Show the store commands read and write, where its path came from, and its state.

Resolves the store exactly as every other command does (`--store`, else MCP_STORE, else the default data location) and reports its absolute `path`, the `source` of that path, and its `state`: `absent` (the first change creates it), `ready` (with the number of `notes`), `needs-migration` (with `pendingMigrations`; run `mcp store migrate`), or `unusable` (with the `problem`). Never creates or changes the store, and reports an unusable store instead of failing.

- **Effect:** reads only
- **Requires:** `store`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--store <path>` | path | no | — | The note store, a SQLite file; overrides MCP_STORE and the default location. `mcp context show` prints the store in effect |

Examples:

```sh
# Show the store in effect
mcp context show
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "version": {
      "type": "string",
      "description": "The mcp version answering."
    },
    "store": {
      "$ref": "#/$defs/StoreContext"
    }
  },
  "required": [
    "version",
    "store"
  ],
  "description": "Where commands take effect.",
  "title": "Context",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "StoreContext": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string",
          "description": "The store's absolute path."
        },
        "source": {
          "$ref": "#/$defs/StoreSource",
          "description": "Where the path came from: `flag` (--store), `environment` (MCP_STORE), or `default`."
        },
        "environmentVariable": {
          "type": "string",
          "description": "The environment variable that selects the store when --store is absent."
        },
        "state": {
          "$ref": "#/$defs/StoreState"
        },
        "notes": {
          "type": [
            "integer",
            "null"
          ],
          "format": "int64",
          "description": "How many notes a `ready` store holds; null otherwise."
        },
        "pendingMigrations": {
          "type": "array",
          "items": {
            "type": "string"
          },
          "description": "Migrations `mcp store migrate` would apply; empty unless `needs-migration`."
        },
        "problem": {
          "type": [
            "string",
            "null"
          ],
          "description": "Why an `unusable` store cannot be used; null otherwise."
        }
      },
      "required": [
        "path",
        "source",
        "environmentVariable",
        "state",
        "pendingMigrations"
      ]
    },
    "StoreSource": {
      "oneOf": [
        {
          "type": "string",
          "const": "flag",
          "description": "`--store`."
        },
        {
          "type": "string",
          "const": "environment",
          "description": "The `MCP_STORE` environment variable."
        },
        {
          "type": "string",
          "const": "default",
          "description": "`$XDG_DATA_HOME/mcp/store.sqlite3`, else `~/.local/share/mcp/store.sqlite3`\n(`%LOCALAPPDATA%\\mcp\\store.sqlite3` on Windows)."
        }
      ],
      "description": "Where the store path came from."
    },
    "StoreState": {
      "oneOf": [
        {
          "type": "string",
          "enum": [
            "ready",
            "unusable"
          ]
        },
        {
          "type": "string",
          "const": "absent",
          "description": "No store yet; the first change creates it."
        },
        {
          "type": "string",
          "const": "needs-migration",
          "description": "`mcp store migrate` must upgrade it before other commands use it."
        }
      ]
    }
  }
}
```

### `notes`

Create, read, update, and delete notes.

#### `mcp notes list`

List notes in name order, one page at a time.

Returns up to --limit notes in name order as `items`, with `hasMore` and `nextCursor`; pass `nextCursor` back as --cursor for the next page, until `hasMore` is false. A store that does not exist yet lists no notes and says so on stderr. Never changes anything.

- **Effect:** reads only
- **Requires:** `store`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--limit <integer>` | integer, 1–100 | no | `20` | The most notes to return |
| `--cursor <name>` | name: `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 64 characters | no | — | Resume after a previous page: the nextCursor it returned |
| `--store <path>` | path | no | — | The note store, a SQLite file; overrides MCP_STORE and the default location. `mcp context show` prints the store in effect |

Examples:

```sh
# List the first page of notes
mcp notes list
# List one note at a time
mcp notes list --limit 1
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/Note"
      }
    },
    "hasMore": {
      "type": "boolean",
      "description": "Whether more items follow this page."
    },
    "nextCursor": {
      "type": [
        "string",
        "null"
      ],
      "description": "Pass as `--cursor` to get the next page; null when there is none."
    }
  },
  "required": [
    "items",
    "hasMore"
  ],
  "description": "One page of a collection.",
  "title": "Page",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "Note": {
      "type": "object",
      "properties": {
        "name": {
          "type": "string",
          "description": "Stable identifier: lowercase letters and digits in hyphen-separated words."
        },
        "title": {
          "type": "string",
          "description": "One line."
        },
        "body": {
          "type": "string",
          "description": "Free text; empty when the note has none."
        },
        "revision": {
          "type": "integer",
          "format": "int64",
          "description": "Starts at 1 and increases on every change. Pass it to `--expect-revision` to change\nonly the version you read."
        },
        "createdAt": {
          "type": "string",
          "description": "When the note was created: RFC 3339, UTC, millisecond precision."
        },
        "updatedAt": {
          "type": "string",
          "description": "When the note last changed: RFC 3339, UTC, millisecond precision."
        }
      },
      "required": [
        "name",
        "title",
        "body",
        "revision",
        "createdAt",
        "updatedAt"
      ],
      "description": "One note as stored."
    }
  }
}
```

#### `mcp notes get`

Read one note by name.

Returns the note with its `revision` and timestamps. Use it to verify a change, or to learn the revision to pass to --expect-revision. Never changes anything.

- **Effect:** reads only
- **Requires:** `store`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<name>` | name: `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 64 characters | yes | — | The note's name |
| `--store <path>` | path | no | — | The note store, a SQLite file; overrides MCP_STORE and the default location. `mcp context show` prints the store in effect |

Examples:

```sh
# Read the note `groceries`
mcp notes get groceries
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "description": "Stable identifier: lowercase letters and digits in hyphen-separated words."
    },
    "title": {
      "type": "string",
      "description": "One line."
    },
    "body": {
      "type": "string",
      "description": "Free text; empty when the note has none."
    },
    "revision": {
      "type": "integer",
      "format": "int64",
      "description": "Starts at 1 and increases on every change. Pass it to `--expect-revision` to change\nonly the version you read."
    },
    "createdAt": {
      "type": "string",
      "description": "When the note was created: RFC 3339, UTC, millisecond precision."
    },
    "updatedAt": {
      "type": "string",
      "description": "When the note last changed: RFC 3339, UTC, millisecond precision."
    }
  },
  "required": [
    "name",
    "title",
    "body",
    "revision",
    "createdAt",
    "updatedAt"
  ],
  "description": "One note as stored.",
  "title": "Note",
  "$schema": "https://json-schema.org/draft/2020-12/schema"
}
```

#### `mcp notes create`

Create a note with a new name.

Creates the note at revision 1. The name must be unused: an existing name fails with CONFLICT and its revision in `details`; change that note with `mcp notes update` instead. Creates the store, with its directory, when it does not exist yet. The receipt reports `status` (`completed`, or `dry-run` when nothing was written), `action` `created`, the `store`, whether the store was created, and the `note` as stored. --dry-run validates the input and checks the name against the store, and creates nothing, not even the store.

- **Effect:** modifies one new note in the store
- **Requires:** `store`
- **Dry run:** yes
- **Idempotent:** no
- **Errors:** `INVALID_ARGUMENT`, `CONFLICT`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--name <name>` | name: `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 64 characters | yes | — | The new note's name, its permanent identifier |
| `--title <string>` | string | yes | — | One line, at most 200 characters |
| `--body <text>` | text | no | empty | The text: the text itself, `@<file>` to read a file, or `-` to read stdin; at most 65536 bytes |
| `--dry-run` | boolean | no | — | Validate input, resolve the store, and return the receipt the command would produce, without writing anything |
| `--store <path>` | path | no | — | The note store, a SQLite file; overrides MCP_STORE and the default location. `mcp context show` prints the store in effect |

Examples:

```sh
# Preview creating a note, writing nothing
mcp notes create --name packing-list --title 'Packing list' --body 'Passport, charger' --dry-run
# Create the note
mcp notes create --name packing-list --title 'Packing list' --body 'Passport, charger'
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "status": {
      "$ref": "#/$defs/ReceiptStatus"
    },
    "action": {
      "$ref": "#/$defs/Action",
      "description": "For `dry-run`, what running without --dry-run would do."
    },
    "store": {
      "type": "string",
      "description": "The store's absolute path."
    },
    "storeCreated": {
      "type": "boolean",
      "description": "Whether the command created a new store: the file, or its schema, did not exist (for\n`dry-run`: would create it)."
    },
    "note": {
      "$ref": "#/$defs/Note",
      "description": "The note after the change; for `deleted`, the note as it was."
    },
    "changed": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The fields the change sets, such as `title`; empty for `unchanged`."
    },
    "previousRevision": {
      "type": [
        "integer",
        "null"
      ],
      "format": "int64",
      "description": "The note's revision before the change; null for `created`."
    },
    "caveats": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "What a dry run cannot promise about the real run; empty when `completed`."
    }
  },
  "required": [
    "status",
    "action",
    "store",
    "storeCreated",
    "note",
    "changed",
    "caveats"
  ],
  "description": "The outcome of a modifying `notes` command.",
  "title": "NoteReceipt",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ReceiptStatus": {
      "oneOf": [
        {
          "type": "string",
          "const": "completed",
          "description": "The change is written."
        },
        {
          "type": "string",
          "const": "dry-run",
          "description": "Nothing was written; the receipt shows what running without --dry-run would do."
        }
      ],
      "description": "Whether a modifying command wrote its change."
    },
    "Action": {
      "oneOf": [
        {
          "type": "string",
          "enum": [
            "created",
            "updated",
            "deleted"
          ]
        },
        {
          "type": "string",
          "const": "unchanged",
          "description": "The requested values equal the stored ones; nothing was written."
        }
      ],
      "description": "What the change did to the note."
    },
    "Note": {
      "type": "object",
      "properties": {
        "name": {
          "type": "string",
          "description": "Stable identifier: lowercase letters and digits in hyphen-separated words."
        },
        "title": {
          "type": "string",
          "description": "One line."
        },
        "body": {
          "type": "string",
          "description": "Free text; empty when the note has none."
        },
        "revision": {
          "type": "integer",
          "format": "int64",
          "description": "Starts at 1 and increases on every change. Pass it to `--expect-revision` to change\nonly the version you read."
        },
        "createdAt": {
          "type": "string",
          "description": "When the note was created: RFC 3339, UTC, millisecond precision."
        },
        "updatedAt": {
          "type": "string",
          "description": "When the note last changed: RFC 3339, UTC, millisecond precision."
        }
      },
      "required": [
        "name",
        "title",
        "body",
        "revision",
        "createdAt",
        "updatedAt"
      ],
      "description": "One note as stored."
    }
  }
}
```

#### `mcp notes update`

Change a note's title or body.

Sets the given fields and leaves the others as stored; pass at least one of --title and --body. A change increments the note's `revision`; values equal to the stored ones change nothing and report `action` `unchanged`, so repeating an update is safe. Pass --expect-revision with the revision you read to fail with CONFLICT instead of overwriting someone else's change. --dry-run reports the same receipt without writing.

- **Effect:** modifies one existing note in the store
- **Requires:** `store`
- **Dry run:** yes
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `CONFLICT`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<name>` | name: `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 64 characters | yes | — | The note's name |
| `--title <string>` | string | no | — | The new title: one line, at most 200 characters |
| `--body <text>` | text | no | — | The new text: the text itself, `@<file>` to read a file, or `-` to read stdin; at most 65536 bytes |
| `--expect-revision <integer>` | integer, at least 1 | no | — | Fail with CONFLICT unless the note is at this revision, so a change never overwrites one you have not read |
| `--dry-run` | boolean | no | — | Validate input, resolve the store, and return the receipt the command would produce, without writing anything |
| `--store <path>` | path | no | — | The note store, a SQLite file; overrides MCP_STORE and the default location. `mcp context show` prints the store in effect |

Examples:

```sh
# Preview a new title, writing nothing
mcp notes update groceries --title 'Groceries for Sunday' --dry-run
# Retitle the note only if nobody changed it since revision 1
mcp notes update groceries --title 'Groceries for Sunday' --expect-revision 1
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "status": {
      "$ref": "#/$defs/ReceiptStatus"
    },
    "action": {
      "$ref": "#/$defs/Action",
      "description": "For `dry-run`, what running without --dry-run would do."
    },
    "store": {
      "type": "string",
      "description": "The store's absolute path."
    },
    "storeCreated": {
      "type": "boolean",
      "description": "Whether the command created a new store: the file, or its schema, did not exist (for\n`dry-run`: would create it)."
    },
    "note": {
      "$ref": "#/$defs/Note",
      "description": "The note after the change; for `deleted`, the note as it was."
    },
    "changed": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The fields the change sets, such as `title`; empty for `unchanged`."
    },
    "previousRevision": {
      "type": [
        "integer",
        "null"
      ],
      "format": "int64",
      "description": "The note's revision before the change; null for `created`."
    },
    "caveats": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "What a dry run cannot promise about the real run; empty when `completed`."
    }
  },
  "required": [
    "status",
    "action",
    "store",
    "storeCreated",
    "note",
    "changed",
    "caveats"
  ],
  "description": "The outcome of a modifying `notes` command.",
  "title": "NoteReceipt",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ReceiptStatus": {
      "oneOf": [
        {
          "type": "string",
          "const": "completed",
          "description": "The change is written."
        },
        {
          "type": "string",
          "const": "dry-run",
          "description": "Nothing was written; the receipt shows what running without --dry-run would do."
        }
      ],
      "description": "Whether a modifying command wrote its change."
    },
    "Action": {
      "oneOf": [
        {
          "type": "string",
          "enum": [
            "created",
            "updated",
            "deleted"
          ]
        },
        {
          "type": "string",
          "const": "unchanged",
          "description": "The requested values equal the stored ones; nothing was written."
        }
      ],
      "description": "What the change did to the note."
    },
    "Note": {
      "type": "object",
      "properties": {
        "name": {
          "type": "string",
          "description": "Stable identifier: lowercase letters and digits in hyphen-separated words."
        },
        "title": {
          "type": "string",
          "description": "One line."
        },
        "body": {
          "type": "string",
          "description": "Free text; empty when the note has none."
        },
        "revision": {
          "type": "integer",
          "format": "int64",
          "description": "Starts at 1 and increases on every change. Pass it to `--expect-revision` to change\nonly the version you read."
        },
        "createdAt": {
          "type": "string",
          "description": "When the note was created: RFC 3339, UTC, millisecond precision."
        },
        "updatedAt": {
          "type": "string",
          "description": "When the note last changed: RFC 3339, UTC, millisecond precision."
        }
      },
      "required": [
        "name",
        "title",
        "body",
        "revision",
        "createdAt",
        "updatedAt"
      ],
      "description": "One note as stored."
    }
  }
}
```

#### `mcp notes delete`

Delete a note permanently.

Removes the note; the receipt carries the note as it was, which is the only copy left. Pass --expect-revision with the revision you read to fail with CONFLICT instead of deleting a note someone changed. Deleting a missing note fails with NOT_FOUND. --dry-run reports the same receipt without deleting.

- **Effect:** modifies one existing note in the store, permanently
- **Requires:** `store`
- **Dry run:** yes
- **Idempotent:** no
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `CONFLICT`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<name>` | name: `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 64 characters | yes | — | The note's name |
| `--expect-revision <integer>` | integer, at least 1 | no | — | Fail with CONFLICT unless the note is at this revision, so a change never overwrites one you have not read |
| `--dry-run` | boolean | no | — | Validate input, resolve the store, and return the receipt the command would produce, without writing anything |
| `--store <path>` | path | no | — | The note store, a SQLite file; overrides MCP_STORE and the default location. `mcp context show` prints the store in effect |

Examples:

```sh
# Preview deleting a note
mcp notes delete standup --dry-run
# Delete the note only if it is still at revision 1
mcp notes delete standup --expect-revision 1
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "status": {
      "$ref": "#/$defs/ReceiptStatus"
    },
    "action": {
      "$ref": "#/$defs/Action",
      "description": "For `dry-run`, what running without --dry-run would do."
    },
    "store": {
      "type": "string",
      "description": "The store's absolute path."
    },
    "storeCreated": {
      "type": "boolean",
      "description": "Whether the command created a new store: the file, or its schema, did not exist (for\n`dry-run`: would create it)."
    },
    "note": {
      "$ref": "#/$defs/Note",
      "description": "The note after the change; for `deleted`, the note as it was."
    },
    "changed": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The fields the change sets, such as `title`; empty for `unchanged`."
    },
    "previousRevision": {
      "type": [
        "integer",
        "null"
      ],
      "format": "int64",
      "description": "The note's revision before the change; null for `created`."
    },
    "caveats": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "What a dry run cannot promise about the real run; empty when `completed`."
    }
  },
  "required": [
    "status",
    "action",
    "store",
    "storeCreated",
    "note",
    "changed",
    "caveats"
  ],
  "description": "The outcome of a modifying `notes` command.",
  "title": "NoteReceipt",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ReceiptStatus": {
      "oneOf": [
        {
          "type": "string",
          "const": "completed",
          "description": "The change is written."
        },
        {
          "type": "string",
          "const": "dry-run",
          "description": "Nothing was written; the receipt shows what running without --dry-run would do."
        }
      ],
      "description": "Whether a modifying command wrote its change."
    },
    "Action": {
      "oneOf": [
        {
          "type": "string",
          "enum": [
            "created",
            "updated",
            "deleted"
          ]
        },
        {
          "type": "string",
          "const": "unchanged",
          "description": "The requested values equal the stored ones; nothing was written."
        }
      ],
      "description": "What the change did to the note."
    },
    "Note": {
      "type": "object",
      "properties": {
        "name": {
          "type": "string",
          "description": "Stable identifier: lowercase letters and digits in hyphen-separated words."
        },
        "title": {
          "type": "string",
          "description": "One line."
        },
        "body": {
          "type": "string",
          "description": "Free text; empty when the note has none."
        },
        "revision": {
          "type": "integer",
          "format": "int64",
          "description": "Starts at 1 and increases on every change. Pass it to `--expect-revision` to change\nonly the version you read."
        },
        "createdAt": {
          "type": "string",
          "description": "When the note was created: RFC 3339, UTC, millisecond precision."
        },
        "updatedAt": {
          "type": "string",
          "description": "When the note last changed: RFC 3339, UTC, millisecond precision."
        }
      },
      "required": [
        "name",
        "title",
        "body",
        "revision",
        "createdAt",
        "updatedAt"
      ],
      "description": "One note as stored."
    }
  }
}
```

### `store`

Prepare and upgrade the note store.

#### `mcp store migrate`

Create the store or upgrade its schema to this mcp's version.

Applies every schema migration the store lacks, oldest first, under the store's write lock, creating the store and its directory when they do not exist. Other commands refuse a store with pending migrations (PRECONDITION_FAILED), so run this after upgrading mcp when they ask for it. The receipt lists the `migrations` applied; an empty list means the store was already current, so running it again is safe. A store written by a newer mcp, or a SQLite database another program wrote, is refused and left unchanged. --dry-run lists the migrations it would apply and changes nothing.

- **Effect:** modifies the store's schema
- **Requires:** `store`
- **Dry run:** yes
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--dry-run` | boolean | no | — | Validate input, resolve the store, and return the receipt the command would produce, without writing anything |
| `--store <path>` | path | no | — | The note store, a SQLite file; overrides MCP_STORE and the default location. `mcp context show` prints the store in effect |

Examples:

```sh
# Show the migrations the store needs, changing nothing
mcp store migrate --dry-run
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "status": {
      "$ref": "#/$defs/ReceiptStatus"
    },
    "store": {
      "type": "string",
      "description": "The store's absolute path."
    },
    "storeCreated": {
      "type": "boolean",
      "description": "Whether the command created a new store: the file, or its schema, did not exist (for\n`dry-run`: would create it)."
    },
    "migrations": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The migrations applied (for `dry-run`: that would be applied), oldest first; empty when\nthe store was current."
    }
  },
  "required": [
    "status",
    "store",
    "storeCreated",
    "migrations"
  ],
  "description": "The outcome of `mcp store migrate`.",
  "title": "MigrationReceipt",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ReceiptStatus": {
      "oneOf": [
        {
          "type": "string",
          "const": "completed",
          "description": "The change is written."
        },
        {
          "type": "string",
          "const": "dry-run",
          "description": "Nothing was written; the receipt shows what running without --dry-run would do."
        }
      ],
      "description": "Whether a modifying command wrote its change."
    }
  }
}
```
