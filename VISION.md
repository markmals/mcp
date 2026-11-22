---
title: A Vision for MCP CLI
updated: 2026-11-22
---

# A Vision for MCP CLI

## Introduction

`mcp` is a Model Context Protocol client for the command line, written for coding agents first. Any agent with a shell can discover, inspect and call the tools, resources and prompts of the MCP servers a project or its user has configured, and compose those calls with pipes, `jq` and scripts the way it composes any other POSIX tool.

Today an MCP server is reachable only through each agent harness's own integration. The harness loads every tool's schema into the model's context up front, a tool's result cannot be piped into another tool or a file, calls cannot be scripted or repeated outside the conversation, each harness reads its own configuration, and a harness without MCP support, or with it turned off, cannot reach the server at all. `mcp` moves MCP behind the interface every agent already has: a command, its arguments, standard input, JSON on standard output, and an exit status.

## Goals

- Any agent with a shell can use any configured MCP server, with no MCP support in its harness and no per-harness configuration.
- An agent spends context only on what its task needs: it searches for a tool, inspects that one tool's contract, and calls it, instead of carrying every server's catalogue.
- MCP calls compose: inputs come from arguments, files or standard input, results are JSON on standard output, and one call's result can feed the next.
- A project declares its servers once, in the `.mcp.json` file other MCP clients already read, and a user declares personal servers once, in one user-level file that applies in every directory.
- A repository's configuration never starts a process or contacts a server until a person has approved it.
- A server behaves across separate `mcp` invocations as it does inside one harness session: a stateful server keeps its state, and a slow server starts once.

## Minimum viable product

In a project with a `.mcp.json` and a user-level configuration, an agent that has never seen `mcp` can, from the installed binary alone:

- find out which servers are configured, where each comes from, and whether it is usable;
- have a person approve the project's servers, with the approval bound to exactly what was approved;
- search, inspect and call tools; list, inspect and read resources; and list and get prompts;
- reach local servers over stdio and remote servers over Streamable HTTP, including servers that require OAuth sign-in;
- add and remove servers in the project or user configuration, previewing each change first;

with server processes kept alive between invocations, JSON results, structured errors with stable exit statuses, and no prompts on the agent's side of the interface.

## Design principles

- **The binary is the documentation** — every operation is discoverable through `cli search`, `schema`, `docs search` and `context show`; a skill or a model's training data is never required to use `mcp` correctly.
- **Pass the server's contract through** — tool, resource and prompt results reach the caller as MCP defines them, with `mcp`'s own metadata beside them rather than mixed into them; `mcp` never summarizes, truncates without saying so, or reinterprets what a server returned.
- **Follow the convention; never half-honor an extension** — `.mcp.json` and its `mcpServers` format are read as other clients write them; a field `mcp` does not implement makes that server fail with an error naming the field, instead of being silently ignored.
- **Consent is a person's act** — approving a repository's servers requires the person's presence (biometrics where the platform has them); an agent following an error's suggestion cannot grant it, and a missing terminal is never consent. Consent takes precedence over convenience and over compatibility with other clients.
- **Context cost scales with the task** — collections are bounded and paginated, search ranks instead of listing everything, and nothing returns every server's full catalogue by default.
- **Secrets never reach output** — environment values, header values and tokens stay out of results, errors, receipts and logs; `mcp` names a missing variable, never a value.

## Beyond the minimum

- Approval from a phone or another device, for example through Sesh's `ask_user`, which matters when the person is away from the machine that would show the biometric prompt.
- Answering server-initiated requests (sampling and elicitation), which needs a model or a person on the other end of the call.
- Long-running tool calls through MCP's Tasks extension, with explicit status and wait commands.
- Streaming server notifications and resource subscriptions as NDJSON.
- Windows, where the server-process daemon would need named pipes instead of Unix domain sockets.
- A deliberate, one-time import of servers from another client's configuration.

## Non-goals

- Being an MCP server, proxy or gateway — harnesses already have MCP clients and servers; the value here is the command-line surface.
- Running a model or choosing tools for the agent — `mcp` is deterministic; the agent decides what to call.
- Installing, updating or sandboxing MCP servers — servers are the user's programs and services; `mcp` runs exactly what the configuration names, and says so.
- Reading other clients' own settings (Claude Code's `~/.claude.json`, Cursor, VS Code, OMP) — loading another tool's configuration silently duplicates servers and runs commands the user configured for a different program.

## Revision history

| Date       | Change               | Related proposal |
| ---------- | -------------------- | ---------------- |
| 2026-11-22 | Created this vision. | N/A              |
