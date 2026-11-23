# CLI reference

`mcp` is an agent-first command-line client for the Model Context Protocol servers configured in a project's `.mcp.json` and the user's `$XDG_CONFIG_HOME/.mcp.json`. This page is generated from the command registry by `mise run cli:reference`; do not edit it by hand.

Every command follows one contract:

- **Results** are JSON on stdout, whether or not stdout is a terminal: compact when piped, indented on a terminal. `--output text` renders the same result for people. Warnings and errors go to stderr.
- **Collections** return `items` and `hasMore`; paged ones also `nextCursor`, which you pass back as `--cursor` until `hasMore` is false. Commands that ask several servers also return `complete` and the `unavailable` servers.
- **Commands that change something** accept `--dry-run`, which validates the input, resolves the target, and returns the receipt the command would produce without changing anything. Receipts carry a `status`: `completed` or `dry-run`.
- **Failures** write one JSON document to stderr and exit with the code's status (see below). Nothing is written to stdout.
- **No command prompts.** Missing input fails at once with `INVALID_ARGUMENT`. Only `servers approve` asks a person, through the system's own authentication dialog.
- **Configuration** is `$XDG_CONFIG_HOME/.mcp.json` (`~/.config/.mcp.json`) and the nearest project `.mcp.json`; `mcp context show` prints both, and `docs/configuration.md` describes them.

Discover a command with `mcp cli search "<task>"`, inspect it with `mcp schema <resource> <operation>`, preview it with `--dry-run`, run it, and verify the result with a read such as `mcp servers info <name>`.

## Options every command accepts

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--output <choice>` | `json`, `text` | no | `json` | Result format: JSON (compact when piped, indented on a terminal) or text for humans |

## Errors

| Code | Exit | Retryable | Meaning |
| --- | --- | --- | --- |
| `INVALID_ARGUMENT` | 2 | no | Input is missing or malformed: an argument, a tool flag, tool input that fails the tool's inputSchema, or parameters the server rejected; `details` says which. |
| `NOT_FOUND` | 3 | no | A named server, tool, command, or document does not exist. |
| `PRECONDITION_FAILED` | 3 | no | A configuration file is invalid, the server's entry is unusable, a variable it needs is unset, MCP_REQUIRE_APPROVAL is invalid, approval is required and missing, or the server has no tools. |
| `PERMISSION_DENIED` | 3 | no | The person declined to approve, could not be authenticated, or could not be asked. |
| `UNAUTHENTICATED` | 3 | no | An HTTP server answered 401 or 403; mcp does not sign in to servers with OAuth. |
| `UNAVAILABLE` | 5 | yes | A server could not be started or reached, the connection was lost, the server answered 429 or 5xx, or the daemon or the approvals file is busy; retry the same command. |
| `TIMEOUT` | 5 | no | No answer within --timeout. A tool call may still have run: verify its effect before retrying, unless the tool declares readOnlyHint or idempotentHint. |
| `TOOL_ERROR` | 6 | no | The tool ran and reported an error (isError); `details.result` holds its whole result. |
| `SERVER_ERROR` | 6 | no | The server answered with a protocol error; `details` carries its code, message, and data. |
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
          "description": "Facts about the failure, such as the argument, the server, or the file."
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
          "description": "A named server, tool, command, or document does not exist.",
          "type": "string",
          "const": "NOT_FOUND"
        },
        {
          "description": "The configuration, the server, or its approval is not ready for the operation.",
          "type": "string",
          "const": "PRECONDITION_FAILED"
        },
        {
          "description": "The person declined approval or could not be asked.",
          "type": "string",
          "const": "PERMISSION_DENIED"
        },
        {
          "description": "An HTTP server refused the request's credentials.",
          "type": "string",
          "const": "UNAUTHENTICATED"
        },
        {
          "description": "A server or the daemon could not be reached; retrying can succeed.",
          "type": "string",
          "const": "UNAVAILABLE"
        },
        {
          "description": "No answer within `--timeout`.",
          "type": "string",
          "const": "TIMEOUT"
        },
        {
          "description": "The tool ran and reported an error.",
          "type": "string",
          "const": "TOOL_ERROR"
        },
        {
          "description": "The server answered with a protocol error.",
          "type": "string",
          "const": "SERVER_ERROR"
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
# Find how to call a tool
mcp cli search 'call a tool'
# Find how to approve a project's servers, three matches at most
mcp cli search 'trust the servers in this repository' --limit 3
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
          "description": "The registry identifier, such as `tools.info`."
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

With no command, lists every command with its summary and effect. With a command path such as `tools info` (or its identifier, `tools.info`), returns that command's full registry entry: each argument with its type, whether it is required, its default, and its accepted values or bounds; whether it reads or modifies state and what it requires; whether it supports --dry-run and is idempotent; the error codes it can return with their exit statuses; examples; and the JSON Schema of its result. Never changes anything.

- **Effect:** reads only
- **Requires:** nothing beyond `mcp`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `NOT_FOUND`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<command>` | string, repeatable | no | — | The command path, such as `tools info`; omit to list every command |

Examples:

```sh
# List every command
mcp schema
# Inspect calling a tool
mcp schema call
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
          "description": "What the command needs besides mcp: `configuration` (the `.mcp.json` files `mcp\ncontext show` reports), `servers` (the configured servers, through the session daemon),\nor `person` (someone at the machine to confirm)."
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
          "description": "`positional`, `option`, `flag`, or `trailing` (every word after the positionals, passed\nthrough to the tool)."
        },
        "usage": {
          "type": "string",
          "description": "How to write it: `<name>`, `--limit <integer>`, `--dry-run`, `[--<property> <value>]...`."
        },
        "type": {
          "type": "string",
          "description": "`string`, `path`, `integer`, `text` (text, `@<file>`, or `-` for stdin), `choice`, or\n`boolean`."
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
          "description": "`reads`, `modifies`, `connects` (starts or reuses server sessions), or `calls` (a\nserver's tool)."
        },
        "what": {
          "type": [
            "string",
            "null"
          ],
          "description": "What the command changes, connects to, or calls."
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

Search mcp's documentation: configuration, approval, sessions, tools, the CLI.

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

Returns the Markdown of one document compiled into this binary, by the `path` that `mcp docs search` reports, such as `docs/tools.md`. `--output text` prints the Markdown alone. Never changes anything.

- **Effect:** reads only
- **Requires:** nothing beyond `mcp`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `NOT_FOUND`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<path>` | string | yes | — | The document, as `docs search` names it: `docs/tools.md` |

Examples:

```sh
# Read how tool flags work
mcp docs show docs/tools.md
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

Show the configuration, workspace, approvals, and daemon in effect.

#### `mcp context show`

Show the configuration files, workspace, approval setting, and session daemon in use.

Reports the workspace and why it was chosen; each configuration file mcp reads (the user's `$XDG_CONFIG_HOME/.mcp.json` and the project's nearest `.mcp.json`) with whether it exists, how many servers it declares, and any problem with it; whether approval is required (MCP_REQUIRE_APPROVAL) and where approvals are kept; and the session daemon's socket, whether it is running, and how many sessions it holds. Works while a configuration file is invalid, and never starts the daemon or a server.

- **Effect:** reads only
- **Requires:** `configuration`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `PRECONDITION_FAILED`, `INTERNAL`

Examples:

```sh
# Show what mcp reads and where
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
    "workspace": {
      "$ref": "#/$defs/WorkspaceContext"
    },
    "files": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/FileContext"
      },
      "description": "The user file, then the project file when one was found."
    },
    "approval": {
      "$ref": "#/$defs/ApprovalContext"
    },
    "daemon": {
      "$ref": "#/$defs/DaemonContext"
    }
  },
  "required": [
    "version",
    "workspace",
    "files",
    "approval",
    "daemon"
  ],
  "title": "Context",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "WorkspaceContext": {
      "type": "object",
      "properties": {
        "path": {
          "type": "string",
          "description": "The directory sessions belong to and stdio servers start in."
        },
        "reason": {
          "$ref": "#/$defs/WorkspaceReason"
        },
        "gitWorktree": {
          "type": [
            "string",
            "null"
          ],
          "description": "The root of the enclosing git worktree; null outside git."
        }
      },
      "required": [
        "path",
        "reason"
      ]
    },
    "WorkspaceReason": {
      "oneOf": [
        {
          "type": "string",
          "const": "project-file",
          "description": "It holds the project file."
        },
        {
          "type": "string",
          "const": "git-worktree",
          "description": "It is the root of the git worktree; there is no project file."
        },
        {
          "type": "string",
          "const": "working-directory",
          "description": "Neither a project file nor a git worktree encloses the working directory."
        }
      ],
      "description": "Why the workspace is the directory it is."
    },
    "FileContext": {
      "type": "object",
      "properties": {
        "source": {
          "$ref": "#/$defs/Source"
        },
        "path": {
          "type": "string"
        },
        "exists": {
          "type": "boolean"
        },
        "servers": {
          "type": "integer",
          "format": "uint",
          "minimum": 0,
          "description": "How many servers it declares."
        },
        "problem": {
          "type": [
            "string",
            "null"
          ],
          "description": "Why it is invalid or unreadable; null when it is fine or absent."
        }
      },
      "required": [
        "source",
        "path",
        "exists",
        "servers"
      ]
    },
    "Source": {
      "oneOf": [
        {
          "type": "string",
          "const": "project",
          "description": "The nearest `.mcp.json` of the working directory's project."
        },
        {
          "type": "string",
          "const": "user",
          "description": "`$XDG_CONFIG_HOME/.mcp.json`, which applies in every directory."
        }
      ],
      "description": "Which configuration file a server comes from."
    },
    "ApprovalContext": {
      "type": "object",
      "properties": {
        "required": {
          "type": "boolean",
          "description": "Whether a project's servers need a person's approval."
        },
        "variable": {
          "type": "string",
          "description": "The environment variable that sets `required`."
        },
        "file": {
          "type": "string",
          "description": "The file approvals are kept in."
        },
        "problem": {
          "type": [
            "string",
            "null"
          ],
          "description": "Why MCP_REQUIRE_APPROVAL or the approvals file is unusable; null when both are fine."
        }
      },
      "required": [
        "required",
        "variable",
        "file"
      ]
    },
    "DaemonContext": {
      "type": "object",
      "properties": {
        "socket": {
          "type": "string"
        },
        "state": {
          "$ref": "#/$defs/DaemonRunState"
        },
        "pid": {
          "type": [
            "integer",
            "null"
          ],
          "format": "uint32",
          "minimum": 0
        },
        "sessions": {
          "type": "integer",
          "format": "uint",
          "minimum": 0,
          "description": "Sessions it holds in every workspace."
        }
      },
      "required": [
        "socket",
        "state",
        "sessions"
      ]
    },
    "DaemonRunState": {
      "oneOf": [
        {
          "type": "string",
          "const": "running",
          "description": "It answers on its socket."
        },
        {
          "type": "string",
          "const": "stopped",
          "description": "Nothing answers; the next command that needs a session starts it."
        }
      ]
    }
  }
}
```

### `servers`

List, inspect, approve, and disconnect configured MCP servers.

#### `mcp servers list`

List the configured MCP servers, where each comes from, and whether it is usable.

Reads the user's `$XDG_CONFIG_HOME/.mcp.json` and the project's nearest `.mcp.json`; a project server replaces the user server of the same name. Each item gives the server's name, its `source` (`project` or `user`) and `file`, its `transport`, its `target` (the command line or URL as written), its `approval` state, whether it is `usable` now and the `problem` when it is not, and whether a session is `connected`. Never starts a server or the session daemon. Fails while either configuration file is invalid.

- **Effect:** reads only
- **Requires:** `configuration`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `PRECONDITION_FAILED`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--limit <integer>` | integer, 1–200 | no | `50` | The most items to return |
| `--cursor <string>` | string | no | — | Resume after a previous page: the nextCursor it returned |

Examples:

```sh
# List every configured server
mcp servers list
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/ServerSummary"
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
    "ServerSummary": {
      "type": "object",
      "properties": {
        "name": {
          "type": "string"
        },
        "source": {
          "$ref": "#/$defs/Source"
        },
        "file": {
          "type": "string",
          "description": "The configuration file that declares it."
        },
        "transport": {
          "anyOf": [
            {
              "$ref": "#/$defs/TransportKind"
            },
            {
              "type": "null"
            }
          ],
          "description": "Null when the entry is too malformed to say."
        },
        "target": {
          "type": [
            "string",
            "null"
          ],
          "description": "The command line or URL as written, before `${VAR}` expansion."
        },
        "approval": {
          "$ref": "#/$defs/ApprovalState"
        },
        "usable": {
          "type": "boolean",
          "description": "Whether mcp can connect to it now."
        },
        "problem": {
          "type": [
            "string",
            "null"
          ],
          "description": "Why it is not usable; null when it is."
        },
        "connected": {
          "type": "boolean",
          "description": "Whether the session daemon holds a session for it in this workspace."
        }
      },
      "required": [
        "name",
        "source",
        "file",
        "approval",
        "usable",
        "connected"
      ],
      "description": "One configured server."
    },
    "Source": {
      "oneOf": [
        {
          "type": "string",
          "const": "project",
          "description": "The nearest `.mcp.json` of the working directory's project."
        },
        {
          "type": "string",
          "const": "user",
          "description": "`$XDG_CONFIG_HOME/.mcp.json`, which applies in every directory."
        }
      ],
      "description": "Which configuration file a server comes from."
    },
    "TransportKind": {
      "type": "string",
      "enum": [
        "stdio",
        "http"
      ],
      "description": "How `mcp` talks to a server."
    },
    "ApprovalState": {
      "oneOf": [
        {
          "type": "string",
          "const": "approved",
          "description": "A person approved this configuration as it is written."
        },
        {
          "type": "string",
          "const": "unapproved",
          "description": "No approval exists for this server in this repository."
        },
        {
          "type": "string",
          "const": "changed",
          "description": "A person approved an earlier configuration of this server."
        },
        {
          "type": "string",
          "const": "not-required",
          "description": "A user-file server, which never needs approval."
        }
      ],
      "description": "Whether a server's configuration, as written, has a person's approval."
    }
  }
}
```

#### `mcp servers info`

Show a server's configuration and what it declares: name, version, capabilities.

Reports everything `servers list` does for one server, plus the names of its `env` and `headers` (never their values), its approval `fingerprint`, and the user file whose server it replaces. When the server is usable, starts or reuses its session and adds `live`: the session and what the server declared (its serverInfo, protocol version, capabilities, instructions, and recent stderr). When it is not, or the connection fails, `live` is null, `problem` says why, and the command still succeeds.

- **Effect:** modifies starts or reuses server sessions; never calls a tool
- **Requires:** `configuration`, `servers`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `PRECONDITION_FAILED`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<name>` | string | yes | — | The server's name, as `mcp servers list` shows it |
| `--timeout <integer>` | integer, 1–3600 | no | `120` | Seconds to wait for each server, starting its session and answering together |

Examples:

```sh
# Inspect the server `demo`
mcp servers info demo
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string"
    },
    "source": {
      "$ref": "#/$defs/Source"
    },
    "file": {
      "type": "string",
      "description": "The configuration file that declares it."
    },
    "transport": {
      "anyOf": [
        {
          "$ref": "#/$defs/TransportKind"
        },
        {
          "type": "null"
        }
      ],
      "description": "Null when the entry is too malformed to say."
    },
    "target": {
      "type": [
        "string",
        "null"
      ],
      "description": "The command line or URL as written, before `${VAR}` expansion."
    },
    "approval": {
      "$ref": "#/$defs/ApprovalState"
    },
    "usable": {
      "type": "boolean",
      "description": "Whether mcp can connect to it now."
    },
    "problem": {
      "type": [
        "string",
        "null"
      ],
      "description": "Why it is not usable; null when it is."
    },
    "connected": {
      "type": "boolean",
      "description": "Whether the session daemon holds a session for it in this workspace."
    },
    "env": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The names of its `env` variables; values are never shown."
    },
    "headers": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "The names of its `headers`; values are never shown."
    },
    "fingerprint": {
      "type": "string",
      "description": "What an approval of this configuration is bound to."
    },
    "replaces": {
      "type": [
        "string",
        "null"
      ],
      "description": "The user file whose server of the same name this project server replaces."
    },
    "live": {
      "anyOf": [
        {
          "$ref": "#/$defs/Live"
        },
        {
          "type": "null"
        }
      ],
      "description": "The session and what the server declared; null when mcp did not connect."
    }
  },
  "required": [
    "name",
    "source",
    "file",
    "approval",
    "usable",
    "connected",
    "env",
    "headers",
    "fingerprint"
  ],
  "description": "One server in full.",
  "title": "ServerDetail",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "Source": {
      "oneOf": [
        {
          "type": "string",
          "const": "project",
          "description": "The nearest `.mcp.json` of the working directory's project."
        },
        {
          "type": "string",
          "const": "user",
          "description": "`$XDG_CONFIG_HOME/.mcp.json`, which applies in every directory."
        }
      ],
      "description": "Which configuration file a server comes from."
    },
    "TransportKind": {
      "type": "string",
      "enum": [
        "stdio",
        "http"
      ],
      "description": "How `mcp` talks to a server."
    },
    "ApprovalState": {
      "oneOf": [
        {
          "type": "string",
          "const": "approved",
          "description": "A person approved this configuration as it is written."
        },
        {
          "type": "string",
          "const": "unapproved",
          "description": "No approval exists for this server in this repository."
        },
        {
          "type": "string",
          "const": "changed",
          "description": "A person approved an earlier configuration of this server."
        },
        {
          "type": "string",
          "const": "not-required",
          "description": "A user-file server, which never needs approval."
        }
      ],
      "description": "Whether a server's configuration, as written, has a person's approval."
    },
    "Live": {
      "type": "object",
      "properties": {
        "session": {
          "$ref": "#/$defs/SessionInfo"
        },
        "server": {
          "$ref": "#/$defs/ServerDeclaration"
        }
      },
      "required": [
        "session",
        "server"
      ]
    },
    "SessionInfo": {
      "type": "object",
      "properties": {
        "id": {
          "type": "string"
        },
        "workspace": {
          "type": "string",
          "description": "The workspace the session belongs to."
        },
        "startedAt": {
          "type": "string",
          "description": "When the session started, RFC 3339."
        },
        "reused": {
          "type": "boolean",
          "description": "Whether an earlier command started this session. False means any state from earlier\ncalls, such as a selected browser page, is gone."
        }
      },
      "required": [
        "id",
        "workspace",
        "startedAt",
        "reused"
      ],
      "description": "The session a result used."
    },
    "ServerDeclaration": {
      "type": "object",
      "properties": {
        "info": {
          "description": "The server's `serverInfo` (`name`, `version`, and the rest), when it sent one."
        },
        "protocolVersion": {
          "type": "string"
        },
        "capabilities": true,
        "instructions": {
          "type": [
            "string",
            "null"
          ]
        },
        "stderr": {
          "type": "array",
          "items": {
            "type": "string"
          },
          "description": "The last lines the server wrote to stderr, oldest first; empty for HTTP servers."
        }
      },
      "required": [
        "protocolVersion",
        "capabilities",
        "stderr"
      ],
      "description": "What the server declared when the session started."
    }
  }
}
```

#### `mcp servers approve`

Ask the person at this machine to approve a project's servers (Touch ID, password).

Asks the person with the system's own authentication (Touch ID, Apple Watch, or the login password on macOS; polkit on Linux) to approve the named project servers, or every one with --all, in one request, then records the approvals in mcp's state directory, bound to this repository and to each server's configuration as written. Always asks, even while approval is not required (MCP_REQUIRE_APPROVAL), so servers can be approved first. An agent cannot approve: a missing or declined answer fails with PERMISSION_DENIED and records nothing. User-file servers return `not-required` and servers already approved `already-approved`, without asking. --dry-run lists what the person would be asked to approve and never asks.

- **Effect:** modifies approvals
- **Requires:** `configuration`, `person`
- **Dry run:** yes
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `PRECONDITION_FAILED`, `PERMISSION_DENIED`, `TIMEOUT`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<names>` | string, repeatable | no | — | The servers' names; or pass --all |
| `--all` | boolean | no | — | Every server instead of the named ones |
| `--dry-run` | boolean | no | — | Validate the input and resolve the target, and return what the command would do, without changing anything |
| `--timeout <integer>` | integer, 1–3600 | no | `120` | Seconds to wait for each server, starting its session and answering together |

Examples:

```sh
# Show what approving the server `demo` would ask the person
mcp servers approve demo --dry-run
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "status": {
      "$ref": "#/$defs/ReceiptStatus"
    },
    "approvalsFile": {
      "type": "string",
      "description": "The file the approvals are kept in."
    },
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/ApprovalItem"
      }
    }
  },
  "required": [
    "status",
    "approvalsFile",
    "items"
  ],
  "title": "ApprovalReceipt",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ReceiptStatus": {
      "oneOf": [
        {
          "type": "string",
          "const": "completed",
          "description": "The change was made."
        },
        {
          "type": "string",
          "const": "dry-run",
          "description": "Nothing changed; the receipt shows what would have happened."
        }
      ],
      "description": "Whether a command changed anything."
    },
    "ApprovalItem": {
      "type": "object",
      "properties": {
        "server": {
          "type": "string"
        },
        "file": {
          "type": "string"
        },
        "outcome": {
          "$ref": "#/$defs/ApprovalOutcome"
        },
        "fingerprint": {
          "type": "string",
          "description": "The configuration approved: what a later change is compared with."
        },
        "target": {
          "type": [
            "string",
            "null"
          ],
          "description": "The command line or URL as written."
        }
      },
      "required": [
        "server",
        "file",
        "outcome",
        "fingerprint"
      ]
    },
    "ApprovalOutcome": {
      "oneOf": [
        {
          "type": "string",
          "const": "approved",
          "description": "The person approved it (with `dry-run`: the person would be asked)."
        },
        {
          "type": "string",
          "const": "already-approved",
          "description": "This configuration was already approved; nothing was asked."
        },
        {
          "type": "string",
          "const": "not-required",
          "description": "A user-file server, which never needs approval."
        }
      ]
    }
  }
}
```

#### `mcp servers revoke`

Remove the approvals of a project's servers.

Removes every approval of the named project servers, or of every server with --all, in this repository. Needs no confirmation. While approval is required (MCP_REQUIRE_APPROVAL=1), also ends the servers' sessions in this workspace. --dry-run reports what would be revoked without changing anything.

- **Effect:** modifies approvals
- **Requires:** `configuration`
- **Dry run:** yes
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<names>` | string, repeatable | no | — | The servers' names; or pass --all |
| `--all` | boolean | no | — | Every server instead of the named ones |
| `--dry-run` | boolean | no | — | Validate the input and resolve the target, and return what the command would do, without changing anything |

Examples:

```sh
# Preview revoking every approval
mcp servers revoke --all --dry-run
# Revoke the server `demo`
mcp servers revoke demo
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "status": {
      "$ref": "#/$defs/ReceiptStatus"
    },
    "approvalsFile": {
      "type": "string"
    },
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/RevocationItem"
      }
    }
  },
  "required": [
    "status",
    "approvalsFile",
    "items"
  ],
  "title": "RevocationReceipt",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ReceiptStatus": {
      "oneOf": [
        {
          "type": "string",
          "const": "completed",
          "description": "The change was made."
        },
        {
          "type": "string",
          "const": "dry-run",
          "description": "Nothing changed; the receipt shows what would have happened."
        }
      ],
      "description": "Whether a command changed anything."
    },
    "RevocationItem": {
      "type": "object",
      "properties": {
        "server": {
          "type": "string"
        },
        "outcome": {
          "$ref": "#/$defs/RevocationOutcome",
          "description": "`revoked`, or `not-approved` when no approval existed."
        },
        "disconnected": {
          "type": "boolean",
          "description": "Whether its session in this workspace ended."
        }
      },
      "required": [
        "server",
        "outcome",
        "disconnected"
      ]
    },
    "RevocationOutcome": {
      "type": "string",
      "enum": [
        "revoked",
        "not-approved"
      ]
    }
  }
}
```

#### `mcp servers disconnect`

End server sessions in this workspace; the next command starts a fresh one.

Ends the sessions of the named servers, or of every server with --all, in this workspace: a stdio server's process is shut down (its stdin closed, then SIGTERM, then SIGKILL). The next command that needs the server starts a new session, without any state the old one held. Never starts the session daemon; with no daemon running, every server is `not-connected`. Works even while a configuration file is invalid.

- **Effect:** modifies sessions
- **Requires:** `servers`
- **Dry run:** yes
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `PRECONDITION_FAILED`, `UNAVAILABLE`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<names>` | string, repeatable | no | — | The servers' names; or pass --all |
| `--all` | boolean | no | — | Every server instead of the named ones |
| `--dry-run` | boolean | no | — | Validate the input and resolve the target, and return what the command would do, without changing anything |

Examples:

```sh
# Preview ending every session in this workspace
mcp servers disconnect --all --dry-run
# End the server `demo`'s session
mcp servers disconnect demo
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "status": {
      "$ref": "#/$defs/ReceiptStatus"
    },
    "workspace": {
      "type": "string"
    },
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/DisconnectItem"
      }
    }
  },
  "required": [
    "status",
    "workspace",
    "items"
  ],
  "title": "DisconnectReceipt",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ReceiptStatus": {
      "oneOf": [
        {
          "type": "string",
          "const": "completed",
          "description": "The change was made."
        },
        {
          "type": "string",
          "const": "dry-run",
          "description": "Nothing changed; the receipt shows what would have happened."
        }
      ],
      "description": "Whether a command changed anything."
    },
    "DisconnectItem": {
      "type": "object",
      "properties": {
        "server": {
          "type": "string"
        },
        "outcome": {
          "$ref": "#/$defs/DisconnectOutcome"
        }
      },
      "required": [
        "server",
        "outcome"
      ]
    },
    "DisconnectOutcome": {
      "oneOf": [
        {
          "type": "string",
          "const": "disconnected",
          "description": "Its session ended (with `dry-run`: would end)."
        },
        {
          "type": "string",
          "const": "not-connected",
          "description": "It had no session in this workspace."
        }
      ]
    }
  }
}
```

### `tools`

List, search, and inspect the tools servers offer.

#### `mcp tools list`

List the tools every usable server offers, one line each.

Starts or reuses each server's session and pages through its tools, in server-name order and then each server's own order. Each item has the server, the tool's name and title, a one-line summary (at most 200 characters; `summaryTruncated` says when it was cut), and the annotations the server declared. Servers that cannot answer (unusable, unapproved, failing to start, timing out) do not fail the command: `complete` is false and `unavailable` lists each with its error code and message. Pass --server to ask only some servers.

- **Effect:** modifies starts or reuses server sessions; never calls a tool
- **Requires:** `configuration`, `servers`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `PRECONDITION_FAILED`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--server <string>` | string, repeatable | no | — | Ask only this server; repeat for several. Default: every configured server |
| `--limit <integer>` | integer, 1–200 | no | `50` | The most tools to return |
| `--cursor <string>` | string | no | — | Resume after a previous page: the nextCursor it returned |
| `--timeout <integer>` | integer, 1–3600 | no | `120` | Seconds to wait for each server, starting its session and answering together |

Examples:

```sh
# List every server's tools
mcp tools list
# List one server's tools, five at a time
mcp tools list --server demo --limit 5
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/ToolSummary"
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
    },
    "complete": {
      "type": "boolean",
      "description": "Whether every server answered."
    },
    "unavailable": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/Unavailable"
      }
    }
  },
  "required": [
    "items",
    "hasMore",
    "complete",
    "unavailable"
  ],
  "description": "One page of a collection.",
  "title": "ToolPage",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ToolSummary": {
      "type": "object",
      "properties": {
        "server": {
          "type": "string"
        },
        "name": {
          "type": "string"
        },
        "title": {
          "type": [
            "string",
            "null"
          ]
        },
        "summary": {
          "type": "string",
          "description": "The description's first line, at most 200 characters."
        },
        "summaryTruncated": {
          "type": "boolean"
        },
        "annotations": {
          "description": "The server's own hints, such as readOnlyHint; unverified."
        }
      },
      "required": [
        "server",
        "name",
        "summary",
        "summaryTruncated"
      ]
    },
    "Unavailable": {
      "type": "object",
      "properties": {
        "server": {
          "type": "string"
        },
        "code": {
          "$ref": "#/$defs/ErrorCode",
          "description": "The error code asking it alone would have reported."
        },
        "message": {
          "type": "string"
        }
      },
      "required": [
        "server",
        "code",
        "message"
      ],
      "description": "A server that could not answer."
    },
    "ErrorCode": {
      "oneOf": [
        {
          "type": "string",
          "const": "INVALID_ARGUMENT",
          "description": "Input is missing or malformed."
        },
        {
          "type": "string",
          "const": "NOT_FOUND",
          "description": "A named server, tool, command, or document does not exist."
        },
        {
          "type": "string",
          "const": "PRECONDITION_FAILED",
          "description": "The configuration, the server, or its approval is not ready for the operation."
        },
        {
          "type": "string",
          "const": "PERMISSION_DENIED",
          "description": "The person declined approval or could not be asked."
        },
        {
          "type": "string",
          "const": "UNAUTHENTICATED",
          "description": "An HTTP server refused the request's credentials."
        },
        {
          "type": "string",
          "const": "UNAVAILABLE",
          "description": "A server or the daemon could not be reached; retrying can succeed."
        },
        {
          "type": "string",
          "const": "TIMEOUT",
          "description": "No answer within `--timeout`."
        },
        {
          "type": "string",
          "const": "TOOL_ERROR",
          "description": "The tool ran and reported an error."
        },
        {
          "type": "string",
          "const": "SERVER_ERROR",
          "description": "The server answered with a protocol error."
        },
        {
          "type": "string",
          "const": "INTERNAL",
          "description": "A defect in `mcp` itself, or a stdout that cannot take the result."
        }
      ],
      "description": "A stable error code. Codes, exit statuses, and retryability are part of the CLI contract."
    }
  }
}
```

#### `mcp tools search`

Find the tools that fit a task described in plain words, across servers.

Ranks every usable server's tools against the query by their names, titles, descriptions, and parameters, locally, and returns the best matches, each with the exact `mcp tools info` command to inspect it. Starts or reuses sessions as `tools list` does, and reports servers that could not answer under `unavailable`.

- **Effect:** modifies starts or reuses server sessions; never calls a tool
- **Requires:** `configuration`, `servers`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `PRECONDITION_FAILED`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<query>` | string, repeatable | yes | — | What you want to do, in plain words; several words need no quotes |
| `--server <string>` | string, repeatable | no | — | Ask only this server; repeat for several. Default: every configured server |
| `--limit <integer>` | integer, 1–50 | no | `10` | The most matches to return |
| `--timeout <integer>` | integer, 1–3600 | no | `120` | Seconds to wait for each server, starting its session and answering together |

Examples:

```sh
# Find a tool that echoes text
mcp tools search 'echo some text'
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "items": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/ToolMatch"
      }
    },
    "hasMore": {
      "type": "boolean",
      "description": "Whether more tools matched than `--limit` allowed."
    },
    "complete": {
      "type": "boolean"
    },
    "unavailable": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/Unavailable"
      }
    }
  },
  "required": [
    "items",
    "hasMore",
    "complete",
    "unavailable"
  ],
  "title": "ToolMatches",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ToolMatch": {
      "type": "object",
      "properties": {
        "server": {
          "type": "string"
        },
        "tool": {
          "type": "string"
        },
        "title": {
          "type": [
            "string",
            "null"
          ]
        },
        "summary": {
          "type": "string"
        },
        "score": {
          "type": "number",
          "format": "double",
          "description": "Relevance from 0 to 1."
        },
        "command": {
          "type": "string",
          "description": "The command that shows the tool's full contract."
        }
      },
      "required": [
        "server",
        "tool",
        "summary",
        "score",
        "command"
      ]
    },
    "Unavailable": {
      "type": "object",
      "properties": {
        "server": {
          "type": "string"
        },
        "code": {
          "$ref": "#/$defs/ErrorCode",
          "description": "The error code asking it alone would have reported."
        },
        "message": {
          "type": "string"
        }
      },
      "required": [
        "server",
        "code",
        "message"
      ],
      "description": "A server that could not answer."
    },
    "ErrorCode": {
      "oneOf": [
        {
          "type": "string",
          "const": "INVALID_ARGUMENT",
          "description": "Input is missing or malformed."
        },
        {
          "type": "string",
          "const": "NOT_FOUND",
          "description": "A named server, tool, command, or document does not exist."
        },
        {
          "type": "string",
          "const": "PRECONDITION_FAILED",
          "description": "The configuration, the server, or its approval is not ready for the operation."
        },
        {
          "type": "string",
          "const": "PERMISSION_DENIED",
          "description": "The person declined approval or could not be asked."
        },
        {
          "type": "string",
          "const": "UNAUTHENTICATED",
          "description": "An HTTP server refused the request's credentials."
        },
        {
          "type": "string",
          "const": "UNAVAILABLE",
          "description": "A server or the daemon could not be reached; retrying can succeed."
        },
        {
          "type": "string",
          "const": "TIMEOUT",
          "description": "No answer within `--timeout`."
        },
        {
          "type": "string",
          "const": "TOOL_ERROR",
          "description": "The tool ran and reported an error."
        },
        {
          "type": "string",
          "const": "SERVER_ERROR",
          "description": "The server answered with a protocol error."
        },
        {
          "type": "string",
          "const": "INTERNAL",
          "description": "A defect in `mcp` itself, or a stdout that cannot take the result."
        }
      ],
      "description": "A stable error code. Codes, exit statuses, and retryability are part of the CLI contract."
    }
  }
}
```

#### `mcp tools info`

Show a tool's description, input and output schemas, annotations, and flags.

Returns the tool exactly as the server declared it (`description`, `inputSchema`, `outputSchema`, `annotations`; annotations such as readOnlyHint are the server's own unverified hints), the `flags` `mcp call` accepts for its input properties, and an example `mcp call` command line.

- **Effect:** modifies starts or reuses server sessions; never calls a tool
- **Requires:** `configuration`, `servers`
- **Dry run:** no
- **Idempotent:** yes
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `PRECONDITION_FAILED`, `UNAUTHENTICATED`, `UNAVAILABLE`, `TIMEOUT`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `<server>` | string | yes | — | The server that offers the tool |
| `<tool>` | string | yes | — | The tool's name, as `mcp tools list` shows it |
| `--timeout <integer>` | integer, 1–3600 | no | `120` | Seconds to wait for each server, starting its session and answering together |

Examples:

```sh
# Inspect the `echo` tool of the server `demo`
mcp tools info demo echo
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "server": {
      "type": "string"
    },
    "session": {
      "$ref": "#/$defs/SessionInfo"
    },
    "tool": {
      "description": "The tool exactly as the server declared it. Its annotations are the server's own\nunverified hints."
    },
    "flags": {
      "type": "array",
      "items": {
        "$ref": "#/$defs/ToolFlag"
      },
      "description": "The flags `mcp call` accepts for the tool's input properties."
    },
    "example": {
      "type": "string",
      "description": "An `mcp call` command line with every required flag."
    }
  },
  "required": [
    "server",
    "session",
    "tool",
    "flags",
    "example"
  ],
  "title": "ToolInfo",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "SessionInfo": {
      "type": "object",
      "properties": {
        "id": {
          "type": "string"
        },
        "workspace": {
          "type": "string",
          "description": "The workspace the session belongs to."
        },
        "startedAt": {
          "type": "string",
          "description": "When the session started, RFC 3339."
        },
        "reused": {
          "type": "boolean",
          "description": "Whether an earlier command started this session. False means any state from earlier\ncalls, such as a selected browser page, is gone."
        }
      },
      "required": [
        "id",
        "workspace",
        "startedAt",
        "reused"
      ],
      "description": "The session a result used."
    },
    "ToolFlag": {
      "type": "object",
      "properties": {
        "flag": {
          "type": "string",
          "description": "How to write it: the property name in kebab-case, or as written when two properties\nshare a kebab-case spelling."
        },
        "property": {
          "type": "string",
          "description": "The property name in the tool's inputSchema; `--<property>` is always accepted too."
        },
        "type": {
          "$ref": "#/$defs/FlagType"
        },
        "itemType": {
          "anyOf": [
            {
              "$ref": "#/$defs/FlagType"
            },
            {
              "type": "null"
            }
          ],
          "description": "How each item of an `array` flag is converted; null otherwise."
        },
        "required": {
          "type": "boolean"
        },
        "description": {
          "type": [
            "string",
            "null"
          ]
        },
        "acceptedValues": {
          "type": [
            "array",
            "null"
          ],
          "items": true,
          "description": "The values the property's `enum` accepts; null when it has none."
        }
      },
      "required": [
        "flag",
        "property",
        "type",
        "required"
      ],
      "description": "One input property, as a flag."
    },
    "FlagType": {
      "oneOf": [
        {
          "type": "string",
          "const": "string",
          "description": "The text unchanged."
        },
        {
          "type": "string",
          "const": "integer",
          "description": "A JSON number."
        },
        {
          "type": "string",
          "const": "number",
          "description": "A JSON number."
        },
        {
          "type": "string",
          "const": "boolean",
          "description": "Alone, `true`; or an explicit `true` or `false`."
        },
        {
          "type": "string",
          "const": "array",
          "description": "Each occurrence adds one item, converted by `itemType`."
        },
        {
          "type": "string",
          "const": "json",
          "description": "JSON text."
        }
      ],
      "description": "How a flag's text becomes a JSON value."
    }
  }
}
```

### `call`

Call a server's tool.

#### `mcp call`

Call a server's tool with its input properties as flags, and print its result.

Starts or reuses the server's session, reads the tool's inputSchema, builds the input from --input (default `{}`) with each flag after the tool setting one property (`--file-path x` or `--filePath x` for `filePath`), checks it against the schema, and calls the tool. mcp's own options go before the server: every word after the tool belongs to the tool. The result is the server's CallToolResult unchanged, with the session that answered; `--output text` prints its text content. A tool that reports an error (isError) fails with TOOL_ERROR and its whole result in `details.result`. --dry-run checks the input and shows what would be sent without calling the tool; the tool's own validation and effects stay unknown until it runs. `--help` after the tool prints its flags.

- **Effect:** modifies a server's tool, whose effects belong to the server
- **Requires:** `configuration`, `servers`
- **Dry run:** yes
- **Idempotent:** no
- **Errors:** `INVALID_ARGUMENT`, `NOT_FOUND`, `PRECONDITION_FAILED`, `UNAUTHENTICATED`, `UNAVAILABLE`, `TIMEOUT`, `TOOL_ERROR`, `SERVER_ERROR`, `INTERNAL`

| Argument | Type | Required | Default | Description |
| --- | --- | --- | --- | --- |
| `--input <text>` | text | no | — | The tool's input as a JSON object, `@<file>`, or `-` for stdin; flags after the tool set or replace its properties |
| `--dry-run` | boolean | no | — | Validate the input and resolve the target, and return what the command would do, without changing anything |
| `--timeout <integer>` | integer, 1–3600 | no | `120` | Seconds to wait for each server, starting its session and answering together |
| `<server>` | string | yes | — | The server that offers the tool |
| `<tool>` | string | yes | — | The tool's name, as `mcp tools list` shows it |
| `[--<property> <value>]...` | string, repeatable | no | — | The tool's input properties as flags, such as `--url https://example.com`; `mcp tools info <server> <tool>` lists them, and `--help` here prints them |

Examples:

```sh
# Call `echo` on the server `demo` with a text property
mcp call demo echo --text hello
# Check the input and show what would be sent, without calling the tool
mcp call --dry-run demo echo --text hello --count 2
# Pass the input as JSON
mcp call --input '{"text":"hello"}' demo echo
```

Result schema:

```json
{
  "type": "object",
  "properties": {
    "status": {
      "$ref": "#/$defs/ReceiptStatus",
      "description": "`completed`, or `dry-run` when the tool was not called."
    },
    "server": {
      "type": "string"
    },
    "tool": {
      "type": "string"
    },
    "session": {
      "$ref": "#/$defs/SessionInfo"
    },
    "inputValidation": {
      "$ref": "#/$defs/InputValidation"
    },
    "arguments": {
      "type": [
        "object",
        "null"
      ],
      "additionalProperties": true,
      "description": "The input that would be sent; present only for `dry-run`."
    },
    "result": {
      "description": "The server's CallToolResult, unchanged: `content`, `structuredContent`, `isError`. Null\nfor `dry-run`."
    },
    "caveats": {
      "type": "array",
      "items": {
        "type": "string"
      },
      "description": "What a dry run cannot tell."
    }
  },
  "required": [
    "status",
    "server",
    "tool",
    "session",
    "inputValidation",
    "caveats"
  ],
  "title": "CallResult",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "ReceiptStatus": {
      "oneOf": [
        {
          "type": "string",
          "const": "completed",
          "description": "The change was made."
        },
        {
          "type": "string",
          "const": "dry-run",
          "description": "Nothing changed; the receipt shows what would have happened."
        }
      ],
      "description": "Whether a command changed anything."
    },
    "SessionInfo": {
      "type": "object",
      "properties": {
        "id": {
          "type": "string"
        },
        "workspace": {
          "type": "string",
          "description": "The workspace the session belongs to."
        },
        "startedAt": {
          "type": "string",
          "description": "When the session started, RFC 3339."
        },
        "reused": {
          "type": "boolean",
          "description": "Whether an earlier command started this session. False means any state from earlier\ncalls, such as a selected browser page, is gone."
        }
      },
      "required": [
        "id",
        "workspace",
        "startedAt",
        "reused"
      ],
      "description": "The session a result used."
    },
    "InputValidation": {
      "type": "object",
      "properties": {
        "status": {
          "$ref": "#/$defs/ValidationStatus",
          "description": "`passed`, or `skipped` when mcp could not compile the tool's inputSchema and sent the\ninput unchecked."
        },
        "reason": {
          "type": [
            "string",
            "null"
          ],
          "description": "Why validation was skipped; null when it passed."
        }
      },
      "required": [
        "status"
      ],
      "description": "Whether `mcp` checked the input before sending it."
    },
    "ValidationStatus": {
      "type": "string",
      "enum": [
        "passed",
        "skipped"
      ]
    }
  }
}
```
