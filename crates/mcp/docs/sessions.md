# Sessions

A session is one connection to one server: a running stdio process, or an HTTP client with any protocol session it negotiated. Sessions outlive the `mcp` invocation that started them, so a server starts once and keeps its state across commands.

## The daemon

Sessions live in a per-user daemon, which the first command that needs a session starts. It is the `mcp` executable itself, running in an internal mode. Its socket is `$XDG_RUNTIME_DIR/mcp/<version>.sock` when `XDG_RUNTIME_DIR` is an absolute path, else `$TMPDIR/mcp-<uid>/<version>.sock`, in a directory only the user can open. Each `mcp` version runs its own daemon. A socket left by a daemon that no longer runs is replaced. `mcp context show` reports the socket, whether the daemon is running, and how many sessions it holds.

## Reuse

- A session belongs to one workspace and one server name; there is at most one for each pair.
- A command reuses the session while the server's configuration, after interpolation, is unchanged. When it changes, the old session ends and a new one starts.
- A server that exits or crashes, or a daemon that restarts, also means a new session.

Every result that used a session carries `session`: its `id`, `workspace`, `startedAt`, and `reused`. `reused: false` means any state from earlier calls, such as a selected browser page, is gone.

## Starting a server

A stdio server starts in the workspace with the environment of the `mcp` invocation that started its session, plus its configured `env`. `mcp` does not install servers: `npx`, `uvx` or the server's own program must be on that environment's `PATH`.

An HTTP server is reached at its `url` with its `headers` on every request. A server that answers 401 or 403 fails with `UNAUTHENTICATED`; `mcp` does not sign in to servers with OAuth.

`--timeout` (default 120 seconds) bounds starting the session and the request together.

## Ending sessions

- A session ends after 30 minutes without a request.
- `mcp servers disconnect <name>` or `--all` ends sessions in this workspace now.
- The daemon exits 60 seconds after its last session ends.

A stdio server is shut down by closing its standard input, then `SIGTERM`, then `SIGKILL`.

## Captured output

A stdio session keeps the last 200 lines (at most 64 KiB) of the server's standard error. `servers info` and errors about the server include them, with interpolated and configured secret values replaced by `<redacted>`.

## What `mcp` tells servers

`mcp` declares no client capabilities: no sampling, elicitation or roots. A server's request for any of them is answered with a method-not-found error. When a server announces that its tools changed, the session's cached tool list is refreshed.

A request that times out, or whose `mcp` process is interrupted, is cancelled: over stdio the server is sent `notifications/cancelled`; over HTTP the response stream is closed.
