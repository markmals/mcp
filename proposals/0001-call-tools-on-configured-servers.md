---
id: proposal.0001
title: Call Tools on Configured MCP Servers
authors: [Mark Malstrom]
status: draft
pull-request: 2
issues: ["https://github.com/markmals/pico/issues/60"]
supersedes: []
---

# Call Tools on Configured MCP Servers

## Summary

Replace the template's notes domain with `mcp`'s foundation: read servers from a project's `.mcp.json` and the user's `~/.config/mcp/.mcp.json`, gate a repository's servers behind a person's biometric approval, keep server sessions alive in a per-user daemon, and search, inspect and call tools over stdio and Streamable HTTP.

## Motivation

An agent can reach an MCP server only through its harness's own MCP integration. That has four costs, each visible in Mark's setup today:

- **Context.** A harness loads every connected server's tool schemas into the model's context at session start, whether or not the task needs them. The agent cannot ask for one tool's contract when it needs it.
- **Composition.** A tool result lands in the conversation. It cannot be piped into `jq`, written to a file, fed to another tool, or repeated from a script.
- **Configuration per harness.** The same `chrome-devtools` server is declared twice on this machine, in `~/Developer/Apps/trove/.mcp.json` and in `~/.claude.json`, and Figma's servers are declared only in OMP's `~/.omp/agent/mcp.json`, so they exist for OMP and nowhere else.
- **Harnesses without MCP.** Pico turns OMP's MCP discovery off and connects only the servers Sesh sends ([pico#60](https://github.com/markmals/pico/issues/60)), so a project's `.mcp.json` servers are unreachable from a Pico session even though Claude Code uses them in the same checkout.

Every one of these agents has a shell. The workaround today is to configure the server again in each harness, or to give up on the server in harnesses that cannot load it.

## Domain grounding

### Established context

- **Tools** ([MCP 2025-11-25, Tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)):
  - `tools/list` is paginated with an opaque `cursor`/`nextCursor`. A tool has `name`, optional `title`, `description`, `inputSchema` (JSON Schema; 2020-12 when `$schema` is absent), optional `outputSchema`, optional `annotations` (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`).
  - Annotations are hints. Clients must treat them as untrusted unless the server is trusted.
  - `tools/call` returns `content` (text, image, audio, resource link, embedded resource), optional `structuredContent`, and `isError`.
  - Failures come in two kinds: protocol errors (JSON-RPC errors such as unknown tool or malformed request) and tool execution errors (`isError: true`). Input validation errors are tool execution errors in this revision.
  - A server that declares `listChanged` sends `notifications/tools/list_changed` when its tools change.
- **Lifecycle** ([MCP 2025-11-25, Lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)): a stdio client shuts a server down by closing its stdin, waiting, then `SIGTERM`, then `SIGKILL`.
- **2026-07-28** ([announcement](https://blog.modelcontextprotocol.io/posts/2026-07-28/)) removes the `initialize` handshake and HTTP sessions from the core protocol; version, client info and capabilities travel with each request.
- **The Rust SDK.** [`rmcp`](https://crates.io/crates/rmcp) 3.11.0 is the official SDK (observed in the crate source):
  - It implements 2026-07-28 as its latest version and 2025-11-25 as the latest version with `initialize`, and negotiates with older servers.
  - It provides a child-process (stdio) client transport and a Streamable HTTP client transport that sends custom headers.
  - It has no client transport for the legacy HTTP+SSE protocol (2024-11-05).
- **The `.mcp.json` convention**, as [pico#60](https://github.com/markmals/pico/issues/60) established it:
  - A top-level `mcpServers` object maps names to servers. `command`, `args`, `env` and `url` are portable; `type` and `headers` are widespread extensions.
  - Claude Code expands `${VAR}` and `${VAR:-default}` in `command`, `args`, `env`, `url` and `headers`.
  - Claude Code treats the file as executable configuration and asks before using a project's servers.
- **Stateful servers.** `chrome-devtools-mcp`'s `select_page` "selects a page as a context for future tool calls" ([tool reference](https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/docs/tool-reference.md)). Two calls made by two separate processes see that context only if they reach the same server process.
- **Biometrics from a command-line process.** On this Mac, a process run from the shell can evaluate LocalAuthentication's `deviceOwnerAuthenticationWithBiometrics`, `deviceOwnerAuthenticationWithBiometricsOrWatch` and `deviceOwnerAuthentication` policies (`canEvaluatePolicy` returned true for each; `biometryType` is Touch ID). [`objc2-local-authentication`](https://crates.io/crates/objc2-local-authentication) binds the framework for Rust.
- **Other programs named `mcp`.** The MCP Python SDK's `cli` extra and MCP Tools (`f/mcptools`) install `mcp` executables; [#1](https://github.com/markmals/mcp/pull/1) documents this in the README.

### Relevant constraints and principles

- [VISION.md](../VISION.md): the binary is the documentation; pass the server's contract through; never half-honor an extension; consent is a person's act; context cost scales with the task; secrets never reach output.
- The template's wire contract stays: JSON on stdout, one structured error on stderr with a stable exit status, no prompts, `--dry-run` on every command that changes state ([registry.rs](../crates/mcp/src/registry.rs), [failure.rs](../crates/mcp/src/failure.rs), [AGENTS.md](../AGENTS.md)).
- **Threat model for approval.** Approval protects against running `mcp` in a repository and unknowingly starting the processes or contacting the URLs that repository configured, including when an agent follows an error's suggestion without reading it. It does not protect against a process that writes `mcp`'s state files directly or runs the configured command itself; such a process already has the access the server would have.

### Quality bar

- An agent with only the binary goes from `mcp --help` to a verified tool call without guessing a command.
- A second invocation reaches the same server process as the first, and the result says so.
- No configured or interpolated secret appears in any error, receipt or captured server output that `mcp` reports, across every failure path the tests exercise.
- No flag, variable or file in the shipped binary approves a repository's server without the person.

### Remaining uncertainty

- The LocalAuthentication prompt has not been shown from an unbundled Rust executable yet, including one started from Sesh with no terminal. Implementation verifies this first; if it fails, this proposal returns to draft.
- [NEEDS CLARIFICATION: How does a person approve on Linux, where there is no LocalAuthentication? Recommendation: `mcp` reads a typed confirmation (the server names) from the controlling terminal, `/dev/tty`, and refuses with `PERMISSION_DENIED` when there is none. This is weaker than biometrics: an agent driving a pseudo-terminal can type it. Alternatives: polkit through `pkcheck` (needs a policy file installed with the binary), or no approval of project servers on Linux at all.]

## Existing baseline

- **Intellisense** (`~/Developer/CLIs/intellisense`) is the closest design.
  - **Preserve:** a daemon as an internal mode of the same executable, keyed by binary version, in a `0700` runtime directory with a `0600` socket; startup locking; recovery from a stale socket; idle expiry; XDG configuration layering; reporting which layer each setting came from; `status` and `limitations` instead of presenting partial evidence as complete.
  - **Compromised:** trust for project configuration is a per-invocation `--trust-project` flag that is never persisted. An agent learns to add the flag every time, so it gates nothing.
- **This repository**, as instantiated from the `agent-cli` template:
  - **Preserve:** the command registry and everything generated from it (parser, `--help`, `schema`, `cli search`, [docs/cli.md](../crates/mcp/docs/cli.md)), the discovery commands, the error document, receipts, and `--dry-run`.
  - **Incidental, and replaced here:** the notes domain, its SQLite store and Diesel, `--store` and `MCP_STORE`, and the Windows archive.
- **MCP Tools** (`f/mcptools`) calls tools from a shell by spawning the server for each invocation (`mcp call <tool> --params '{…}' npx …`). It shows the demand, and the cost: every call pays the server's startup and loses its state.
- **Claude Code** reads `.mcp.json` from the directory it is started in and asks once per project. OMP's reader accepts both `mcp.json` and `.mcp.json` and leaves an unset variable as literal text without saying so (pico#60). The second is the behavior this proposal avoids.

## Proposed solution

`mcp` reads two files with the same name and format: the nearest project `.mcp.json` and the user's `$XDG_CONFIG_HOME/mcp/.mcp.json`. A project server never starts until a person approves it with Touch ID. The approval is bound to the repository and to the server's configuration as written. Sessions live in a per-user daemon, so a server starts once and keeps its state across invocations. Agents work through `servers` and `tools` commands that follow the template's discover, inspect, preview, run and verify loop.

```text
mcp tools search "take a screenshot"           → ranked tools across servers, each with its `tools get` command
mcp tools call chrome-devtools take_snapshot   → PRECONDITION_FAILED (exit 3): project server not approved;
                                                 suggestion: a person runs `mcp servers approve chrome-devtools`
mcp servers approve chrome-devtools            → Touch ID dialog naming the server and its command → receipt
mcp tools call chrome-devtools navigate_page --input '{"url":"https://example.com"}'
                                               → {"server":…,"tool":…,"session":{"reused":false,…},"result":{…}}
mcp tools call chrome-devtools take_snapshot --output text | rg 'Example Domain'
                                               → same page: session reused
```

## Detailed design

### Configuration ([docs/configuration.md](../crates/mcp/docs/configuration.md), new)

- **Files.**
  - User file: `$XDG_CONFIG_HOME/mcp/.mcp.json` when `XDG_CONFIG_HOME` is an absolute path, else `~/.config/mcp/.mcp.json`, on macOS too.
  - Project file: the nearest `.mcp.json` from the working directory upward, stopping at the root of the enclosing git worktree. Outside a git worktree, only the working directory is searched.
  - A project file that resolves to the user file is the user file.
  - Only one project file applies; nested files never merge.
- **Workspace.** The directory holding the project file, else the git worktree root, else the working directory. Sessions are scoped to it, and stdio servers start in it.
- **Format.** A JSON object with `mcpServers` (an object) and optionally `$schema`.
  - Any other top-level key, invalid JSON, or a non-object `mcpServers` makes the file invalid.
  - An invalid or unreadable file makes every command that resolves servers fail with `PRECONDITION_FAILED`, naming the file and the cause. Only the discovery commands and `context show` keep working. This stops a broken project file from silently letting a user server of the same name answer instead.
- **Server entry fields.**

  | Field     | Rule                                                                                                                      |
  | --------- | ------------------------------------------------------------------------------------------------------------------------- |
  | `type`    | `"stdio"` or `"http"`; inferred from `command` or `url` when absent. `"sse"` is recognized and unsupported.               |
  | `command` | Nonempty string. Stdio only. Run directly, never through a shell; a relative path resolves against the workspace.         |
  | `args`    | Array of strings. Stdio only.                                                                                             |
  | `env`     | Object of strings, added to the environment of the invocation that starts the session. Stdio only. Never shell-evaluated. |
  | `url`     | Absolute `http` or `https` URL after interpolation. HTTP only.                                                            |
  | `headers` | Object of strings. HTTP only.                                                                                             |

- **Entry validation.**
  - A server needs exactly one of `command` and `url`, and its name must be nonempty with no control characters.
  - Any other field, a field for the wrong transport, or a wrong type makes **that server** unusable with `PRECONDITION_FAILED`. The error names the file, the server and the field. Other servers keep working.
  - An unusable project server still replaces a user server of the same name.
- **Precedence.** A project server replaces the whole user server of the same name; nothing merges. `servers get` reports what it replaced.
- **Interpolation.**
  - `${VAR}` and `${VAR:-default}` expand in `command`, `args`, `env` values, `url` and `headers` values. They expand from the environment of the current invocation, every time.
  - `:-` follows the shell: an empty variable takes the default.
  - An unset variable with no default makes the server unusable with `PRECONDITION_FAILED`, naming the variable and never a value.
  - A `$` not followed by `{` is literal, and a value starting with `!` is literal text.
- **Display.** Every output shows configuration as written, before interpolation. `env` and `headers` appear as names only.

### Approval ([docs/approval.md](../crates/mcp/docs/approval.md), new)

- **What needs approval.** Every server from a project file, stdio and HTTP alike. User-file servers are the user's own and need none (`approval: "not-required"`).
- **Fingerprint.** SHA-256 of the server's entry as written, canonicalized (sorted keys, no insignificant whitespace). Changing a `${VAR}`'s value needs no new approval; changing the reference, a command, an argument, a URL or a literal value does.
- **Binding.** An approval binds four things:
  - the repository: the canonical git common directory, so every worktree of a clone shares approvals and another clone does not; outside git, the file's directory;
  - the project file's path relative to the worktree root;
  - the server name;
  - the fingerprint.

  Several fingerprints can be approved for one server, so switching branches does not require approving again.
- **Store.** `$XDG_STATE_HOME/mcp/approvals.json`, else `~/.local/state/mcp/approvals.json`, mode `0600`, replaced atomically under a lock. It records the binding, the command or URL as written, and the approval time; it holds no `env` or `headers` values.
- **States.** `approved`, `unapproved` (never approved), `changed` (approved under another fingerprint) and `not-required`.
- **Asking the person.**
  - On macOS, `servers approve` evaluates LocalAuthentication's `deviceOwnerAuthentication` policy: Touch ID, Apple Watch, or the login password. The dialog's reason names each server and its command or URL.
  - [NEEDS CLARIFICATION: Should approval accept the login password, or only biometrics and Apple Watch (`deviceOwnerAuthenticationWithBiometricsOrWatch`)? Recommendation: accept the password, so approval still works with the lid closed and no watch; Touch ID is what the person sees whenever it is available.]
  - Cancellation, failure, or no graphical session (for example over SSH) fails with `PERMISSION_DENIED` and records nothing.
  - The prompt is withdrawn after `--timeout` seconds (default 120), which fails with `TIMEOUT`.
  - Linux follows the open question in Domain grounding.
- **No bypass.** The binary has no flag, variable or file format that approves without the person. Tests seed `approvals.json` directly; `approve` runs in tests only with `--dry-run`.
- **Using an unapproved server.** A command that would connect to an `unapproved` or `changed` project server fails with `PRECONDITION_FAILED`. The details carry `server`, `file` and `approval`. The suggestion names `mcp servers approve <name>` and says a person must confirm it.

### Sessions ([docs/sessions.md](../crates/mcp/docs/sessions.md), new)

- **The daemon.**
  - A session is one connection to one server: a running stdio process, or an HTTP client with any protocol session it negotiated.
  - Sessions live in a per-user daemon, an internal mode of the `mcp` executable that is absent from the command registry. Its socket is `$XDG_RUNTIME_DIR/mcp/<version>.sock`, else `$TMPDIR/mcp-<uid>/<version>.sock`, in a `0700` directory with a `0600` socket.
  - The first command that needs it starts it. A stale socket is replaced.
- **Session identity.**
  - At most one session exists per workspace and server name.
  - A session is reused while the server's configuration after interpolation is unchanged. When it changes, the old session ends and a new one starts.
  - A crashed server or a restarted daemon also means a new session.
- **Stdio servers** start in the workspace with the environment of the `mcp` invocation that started the session, plus the server's `env`. Shutdown follows the spec: close stdin, then `SIGTERM`, then `SIGKILL`.
- **Expiry.** A session ends after 30 minutes without a request. The daemon exits 60 seconds after its last session ends.
- **Captured output.** Each stdio session keeps its last 200 lines of server stderr (at most 64 KiB). Failures and `servers get` include it after redaction.
- **Client capabilities.** `mcp` declares none: no sampling, elicitation or roots. A server-initiated request is answered with a JSON-RPC method-not-found error. A tools `list_changed` notification refreshes the session's cached tool list.
- **Cancellation.** A request that times out or whose `mcp` process is interrupted is cancelled: `notifications/cancelled` over stdio, a closed stream over HTTP.
- **Redaction.** Before `mcp` reports any error message, detail or captured server output, it replaces every interpolated value and every literal `env` or `headers` value of eight characters or more with `<redacted>`. Tool results are the server's contract and pass through unredacted.
- **Session metadata.** Every result that used a session carries `session`: `id`, `workspace`, `startedAt`, and `reused`. `reused: false` tells the agent that any state from earlier calls is gone.

### Commands ([registry.rs](../crates/mcp/src/registry.rs), [commands/](../crates/mcp/src/commands/), generated [docs/cli.md](../crates/mcp/docs/cli.md))

- **Removed:** `notes list|get|create|update|delete`, `store migrate`, `--store`, `MCP_STORE`, `Requirement::Store`, the name pattern, Diesel, the migrations and the store.
- **Kept:** `cli search`, `schema`, `docs search`, `docs show`, `--version`, `--output json|text`.
- **New effects.** `Effect` gains two variants beside `Reads` and `Modifies`, and `schema` and `cli search` report them:
  - `Connects`: starts or reuses sessions of usable, approved servers, never calls a tool, and needs no `--dry-run`.
  - `Calls`: calls a server's tool, whose effects belong to the server. `tools get` reports the tool's annotations, labeled as the server's unverified hints.
- **`--timeout <seconds>`** (default 120, 1–3600) on every command that contacts a server. It covers starting the session and the request.
- **`--server <name>`** (repeatable) restricts multi-server commands.

| Command                                                                         | Effect                           | Result                                                                                                                                                                                                                                        |
| ------------------------------------------------------------------------------- | -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `context show`                                                                  | Reads                            | Version; workspace and why it was chosen; each configuration file's path, existence and problem; the approvals store; the daemon's socket, state and session count. Replaces the store report.                                                |
| `servers list [--limit 50] [--cursor]`                                          | Reads, local only                | Page of servers: `name`, `source` (`project`/`user`), `file`, `transport`, `target` (command line or URL as written), `approval`, `usable`, `problem`, `connected`.                                                                           |
| `servers get <name>`                                                            | Reads, local only                | Everything in the list entry, plus `env` and `headers` names, `fingerprint`, what it replaces, and the live session if any: server name, version, negotiated protocol version, capabilities, instructions, recent stderr.                     |
| `servers approve <name>… \| --all [--dry-run] [--timeout]`                      | Modifies approvals; needs person | Receipt per server: `approved`, `already-approved` or `not-required`, with fingerprint and target. `--dry-run` lists what the person would be asked to approve and never prompts.                                                             |
| `servers revoke <name>… \| --all [--dry-run]`                                   | Modifies approvals               | Receipt per server: `revoked` or `not-approved`. Ends the server's sessions in this workspace.                                                                                                                                                |
| `servers connect <name> [--dry-run] [--timeout]`                                | Connects                         | The live session and what the server declared (as in `servers get`). `--dry-run` reports `would-start` or `would-reuse` and the target, starting nothing.                                                                                     |
| `servers disconnect <name>… \| --all [--dry-run]`                               | Modifies sessions                | Receipt per server: `disconnected` or `not-connected`.                                                                                                                                                                                        |
| `tools list [--server]… [--limit 50] [--cursor] [--timeout]`                    | Connects                         | Page of tools in server-name order, then the server's order: `server`, `name`, `title`, `summary` (first line, at most 200 characters, with `summaryTruncated`), declared annotations. Plus `complete` and `unavailable` (see below).         |
| `tools search <query> [--server]… [--limit 10] [--timeout]`                     | Connects                         | Up to 50 tools ranked by the same local keyword scorer as `cli search`, over server and tool names, titles, descriptions and parameter names and descriptions. Each carries the exact `tools get` command. Plus `complete` and `unavailable`. |
| `tools get <server> <tool> [--timeout]`                                         | Connects                         | The tool as the server declared it (`description`, `inputSchema`, `outputSchema`, `annotations`), plus an example `tools call` command line.                                                                                                  |
| `tools call <server> <tool> [--input <json>\|@file\|-] [--dry-run] [--timeout]` | Calls                            | `{server, tool, session, result}`, where `result` is the server's `CallToolResult` unchanged. `--output text` prints the text content blocks and one line per non-text block (type, MIME type, size).                                         |

- **`complete` and `unavailable`.** A multi-server command still succeeds when some servers cannot answer, but says so. `complete` is `false` and `unavailable` lists each such server with its error `code` and `message`: unapproved, unusable, failed to start, timed out.
- **`tools call` input.**
  - `--input` is a JSON object, `@path` to read one from a file, or `-` to read one from stdin; it defaults to `{}`.
  - The input is validated against the tool's `inputSchema` before sending. A failure is `INVALID_ARGUMENT`, with each violation's instance path and keyword in `details`.
  - If `mcp` cannot compile the schema, it sends the input unvalidated and the result carries `inputValidation: {"status": "skipped", "reason": …}`. Otherwise `inputValidation.status` is `passed`.
  - `--dry-run` starts or reuses the session to read the tool's schema, validates the input, and returns what it would send without calling the tool. Its caveat says the tool's own validation and effects are unknown until it runs.

### Errors ([failure.rs](../crates/mcp/src/failure.rs))

| Code                  | Exit | Retryable | Meaning                                                                                                                                                                               |
| --------------------- | ---- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `INVALID_ARGUMENT`    | 2    | no        | Malformed CLI input; tool input that fails its `inputSchema`; a server's JSON-RPC invalid-params error.                                                                               |
| `NOT_FOUND`           | 3    | no        | Unknown server, tool, command or document.                                                                                                                                            |
| `PRECONDITION_FAILED` | 3    | no        | Invalid configuration file, unusable server, unset variable, unapproved or changed project server, or a server without the tools capability.                                          |
| `PERMISSION_DENIED`   | 3    | no        | The person declined approval, could not be authenticated, or could not be asked.                                                                                                      |
| `UNAUTHENTICATED`     | 3    | no        | An HTTP server answered 401 or 403. Sign-in (OAuth) is not supported by this proposal; the message says so.                                                                           |
| `UNAVAILABLE`         | 5    | yes       | The server failed to start or connect, the connection was lost, HTTP 429 or 5xx, or the daemon or approvals store is busy.                                                            |
| `TIMEOUT`             | 5    | no        | No answer within `--timeout`. For `tools call` the tool may have run; suggestions say to verify before retrying, or to retry a tool that declares `readOnlyHint` or `idempotentHint`. |
| `TOOL_ERROR`          | 6    | no        | The tool returned `isError: true`. `details.result` holds the whole `CallToolResult`. Stdout is empty.                                                                                |
| `SERVER_ERROR`        | 6    | no        | Any other JSON-RPC error from the server. `details` carries its `code`, `message` and `data`.                                                                                         |
| `INTERNAL`            | 70   | no        | A defect in `mcp`.                                                                                                                                                                    |

`CONFLICT` is removed; nothing in this proposal can conflict.

### Repository changes

- **Shipped documentation.** [docs/notes.md](../crates/mcp/docs/notes.md) and [docs/store.md](../crates/mcp/docs/store.md) are replaced by `configuration.md`, `approval.md`, `sessions.md` and `tools.md`, listed in `docs::DOCUMENTS`. These documents, compiled into the binary for `docs search`, are the user guide; `guides/` gains nothing.
- **[README.md](../README.md) and [AGENTS.md](../AGENTS.md):** describe `mcp`, its files, approval and sessions; replace the notes examples and the `MCP_STORE` reviewer recipe. The new recipe uses a temporary `XDG_CONFIG_HOME`, `XDG_STATE_HOME` and `XDG_RUNTIME_DIR` with the test server.
- **Windows is dropped**, because the daemon uses Unix domain sockets:
  - [build-archives.yml](../.github/workflows/build-archives.yml) and [ci.yml](../.github/workflows/ci.yml): remove the `x86_64-pc-windows-msvc` and `windows-latest` jobs.
  - [knope.toml](../knope.toml): remove the `.zip` asset.
  - [xtask/src/main.rs](../xtask/src/main.rs): remove the Windows packaging.
  - [mise.toml](../mise.toml): remove `windows-x64` from `lockfile_platforms` and regenerate `mise.lock`.
  - README: name four archives.
- **[.env.schema](../.env.schema):** remove `MCP_STORE`. **[.agents/commit-scopes](../.agents/commit-scopes):** replace `notes` and `store` with `config`, `approval`, `sessions` and `tools`.
- **Dependencies:**
  - Remove: `diesel`, `diesel_migrations`, `libsqlite3-sys`.
  - Add: `rmcp` (client, child-process and Streamable HTTP transports, rustls), `tokio`, `sha2`, and `objc2-local-authentication` on macOS.
  - Move `jsonschema` from a development dependency to a dependency.
- **Test server.** A development-only MCP server in this workspace, built on `rmcp`'s server feature. Every registered example and the process tests run against it over stdio and Streamable HTTP. Its tools:
  - echo with structured content;
  - one that returns `isError`;
  - a per-process counter that proves session reuse;
  - a slow tool for timeouts;
  - a header check for HTTP.

  Tests also assert that a sentinel secret placed in `env`, `headers` and an interpolated variable never appears on stdout or stderr in any failure path.

## Compatibility

No compatibility impact. Nothing has been released. The template's notes commands, store, `MCP_STORE` and the Windows archive are removed before any user depends on them.

Existing `.mcp.json` files written for Claude Code work unchanged when they use the portable fields plus `type` and `headers`. A file that uses another client's extension (OMP's `auth`, a `cwd`, `type: "sse"`) is reported field by field rather than half-honored.

## Implications on adoption

- macOS 10.15 or newer for LocalAuthentication's policies; Linux per the open question; no Windows build.
- `mcp` does not install servers. `npx`, `uvx` or a server's own binary must be on `PATH` in the environment of the invocation that starts the session.
- A project's servers need one approval per repository from the person, again whenever their configuration changes.
- Reversible: deleting the binary, `~/.config/mcp/` and `~/.local/state/mcp/` removes every trace. A daemon left running exits after its sessions expire.

## Scope

- Configuration discovery, validation, interpolation and precedence; `context show`.
- Approval with LocalAuthentication on macOS (and Linux, per the open question); `servers approve` and `servers revoke`.
- The session daemon over stdio and Streamable HTTP with static headers; `servers list`, `get`, `connect` and `disconnect`.
- `tools list`, `search`, `get` and `call`; the error codes above.
- Removing the notes domain and the Windows target; the dependency changes; shipped documentation, README, AGENTS.md and the regenerated reference.
- The test server, process tests for every command and error path, the secret-sentinel tests, and the registry drift tests the template already has.

### Out of scope

The first three items complete the vision's minimum viable product, each in its own proposal:

- **Resources and prompts.** They reuse this proposal's configuration, approval and sessions, but add their own commands and content handling.
- **OAuth sign-in for remote servers.** It needs a browser flow, a callback listener and token storage in the Keychain. Until then, a server that needs it fails with `UNAUTHENTICATED`.
- **Editing configuration (`servers add`, `servers remove`).** It writes files and needs its own receipts and rules for which file it changes.

The rest is beyond the minimum viable product:

- **Windows**, which needs named pipes for the daemon. See VISION.md, Beyond the minimum.
- **Sampling, elicitation and roots; the Tasks extension; streaming notifications.** See VISION.md, Beyond the minimum.
- **`cwd`, `type: "sse"`, and other clients' extensions.** They are recognized and refused, never half-honored.
- **A `--workspace` override.** Running `mcp` in the intended directory is sufficient for the first version.

## Preview

- **Artifact:** an installable executable. The `aarch64-apple-darwin` archive is built by Actions → Build archives on this branch and downloaded with `gh run download`; the repository is private.
- **Exercise:**
  - In `~/Developer/Apps/trove`, the person approves `chrome-devtools` with Touch ID. They list, search and call its tools across separate invocations, and the selected page persists between them.
  - A user file holding the Figma desktop server (`http://127.0.0.1:3845/mcp`) is reachable from any directory.
  - Changing the project server's arguments returns it to `changed`.
- **Cleanup:** `mcp servers revoke --all` in trove; remove any preview entries from `~/.config/mcp/.mcp.json`.
- **Reason:** biometric approval, a real stateful server and a real HTTP server can be exercised only with the built binary on Mark's Mac.

## Policies and decisions checked

- None. `policies/` and `decisions/` hold only their READMEs.

## Future directions

- Per-property flags generated from a tool's `inputSchema` (`--url https://…`) as an alternative to `--input` JSON.
- Approval from another device through Sesh's `ask_user`, for a person who is away from the Mac showing the prompt.
- Writing large binary content blocks to files instead of inline base64.
- A configurable session idle time.

## Alternatives considered

- **Spawn the server for every invocation** (MCP Tools' model). It is simpler, with no daemon, but a stateful server loses its state between calls and a slow server pays its startup every time. That breaks `chrome-devtools-mcp`, the server Mark already uses.
- **A per-invocation `--trust-project` flag** (Intellisense). Agents add it to every call, so it records no decision.
- **Persisted approval without asking the person.** An agent following the error's suggestion would approve the repository's servers itself.
- **No approval** (Claude Code's non-interactive runs). This runs any cloned repository's commands on the first `tools list`.
- **Reading other clients' files** (OMP's `loadAllMCPConfigs`). VISION.md excludes it: it duplicates servers and runs commands configured for another program.
- **One command per tool** (`mcp chrome-devtools navigate-page --url …`). That surface is dynamic and cannot live in the static registry that generates `schema`, `cli search` and the reference. `tools get` already gives each tool's contract, and JSON input composes with `jq` and files.
- **Return `CallToolResult` bare, without an envelope.** That saves `.result` in a `jq` path, but loses which session answered and whether earlier state survived.
- **One daemon per workspace** (Intellisense keys daemons by workspace). One per user and version keeps one socket to find and report; sessions are still isolated per workspace.

## Open questions

- [NEEDS CLARIFICATION: Linux approval mechanism; see Remaining uncertainty.]
- [NEEDS CLARIFICATION: Password fallback for approval on macOS; see Approval.]
