# Configuration

`mcp` reads MCP servers from two files with the same name and format: the user file, which applies in every directory, and the nearest project file. `mcp context show` reports both paths, whether each exists, and any problem with it.

## Files

- **User file:** `$XDG_CONFIG_HOME/.mcp.json` when `XDG_CONFIG_HOME` is an absolute path, else `~/.config/.mcp.json`. macOS uses the same path.
- **Project file:** the nearest `.mcp.json` from the working directory upward, stopping at the root of the enclosing git worktree. Outside a git worktree only the working directory is searched. Only one project file applies; nested files never merge.
- A project file that is the same file as the user file counts as the user file.

## Workspace

The workspace is the directory holding the project file, else the root of the git worktree, else the working directory. Server sessions belong to a workspace, and stdio servers start in it. `context show` reports the workspace and why it was chosen: `project-file`, `git-worktree` or `working-directory`.

## Format

The file is the `mcpServers` format other MCP clients read:

```json
{
    "mcpServers": {
        "chrome-devtools": {
            "command": "npx",
            "args": ["-y", "chrome-devtools-mcp@latest"]
        },
        "figma": {
            "type": "http",
            "url": "http://127.0.0.1:3845/mcp",
            "headers": { "Authorization": "Bearer ${FIGMA_TOKEN}" }
        }
    }
}
```

The top level is an object holding `mcpServers` (an object) and optionally `$schema`. Any other top-level key, invalid JSON, or an `mcpServers` that is not an object makes the file invalid. While a file is invalid or unreadable, every command that resolves servers fails with `PRECONDITION_FAILED`, naming the file and the cause; `cli`, `schema`, `docs` and `context show` keep working. A broken project file therefore never lets a user server of the same name answer in its place.

### Server fields

| Field     | Rule                                                                                                                                                      |
| --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `type`    | `"stdio"` or `"http"`. Inferred from `command` or `url` when absent. `"sse"` (the legacy HTTP+SSE transport) is recognized and not supported.             |
| `command` | Nonempty string. Stdio only. Run directly, never through a shell. A relative path containing `/` resolves against the workspace; a bare name uses `PATH`. |
| `args`    | Array of strings. Stdio only.                                                                                                                             |
| `env`     | Object of strings, added to the environment of the `mcp` invocation that starts the session. Stdio only. Values are never shell-evaluated.                |
| `url`     | Absolute `http` or `https` URL once interpolated. HTTP only.                                                                                              |
| `headers` | Object of strings sent with every request. HTTP only.                                                                                                     |

A server needs exactly one of `command` and `url`. Its name must be nonempty and contain no control characters.

Any other field, a field that belongs to the other transport, or a value of the wrong type makes that server unusable: commands that need it fail with `PRECONDITION_FAILED`, and the error names the file, the server and the field. Other servers keep working. `mcp` never ignores a field it does not implement, such as `cwd`, another client's `auth`, or `type: "sse"`.

## Precedence

A project server replaces the user server with the same name, whole: nothing merges. An unusable project server still replaces it. `mcp servers info <name>` reports the file a server came from and the user file it replaced.

## Interpolation

`${VAR}` and `${VAR:-default}` expand in `command`, `args`, `env` values, `url` and `headers` values, from the environment of the current `mcp` invocation, every time a command runs.

- `${VAR:-default}` follows the shell: an empty variable also takes the default.
- An unset variable with no default makes the server unusable with `PRECONDITION_FAILED`. The error names the variable, never a value.
- A `$` not followed by `{` is literal. A value starting with `!` is literal text; `mcp` never runs it.

## What output shows

Every result shows configuration as written in the file, before interpolation. `env` and `headers` appear as names only, never values. Errors and captured server output replace each interpolated value, and each literal `env` or `headers` value, of eight characters or more with `<redacted>`.
