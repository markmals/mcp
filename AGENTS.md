# MCP CLI

This project follows the Workbench process. Workbench loads from `.workbench`, a gitignored link that `mise install` restores: when `.workbench` is missing or dangling, the harness loads no Workbench content and tasks that call `workbench` fail, so run `mise install`.

This file holds only what is true of this project. The process itself comes from the Workbench package.

## Purpose and layout

`mcp` is an agent-first command-line client for Model Context Protocol servers, in Rust: an unfamiliar agent discovers, inspects, previews, runs and verifies every operation from the installed binary. It reads servers from a project's `.mcp.json` and the user's `$XDG_CONFIG_HOME/.mcp.json`, keeps their sessions in a per-user daemon, and calls their tools with the tools' input properties as flags.

- `crates/mcp/`: the shipped CLI. `src/registry.rs` declares the command contract; `src/commands/` holds each resource's registry entries and implementations; `src/config/` (configuration files), `src/approval/` (approval and asking the person), `src/daemon/` (the session daemon and its client), `src/connect.rs` and `src/tool_flags.rs` are the domain; `docs/` is the Markdown compiled into the binary for `docs search` (`docs/cli.md` is generated); `examples/mcp-test-server/` is the MCP server the tests run against; `tests/cli/` runs the real binary.
- `xtask/`: the unpublished release helper, `cargo xtask package` and `cargo xtask formula`.
- `.agents/skills/`: `designing-agent-first-clis` (the CLI contract) and `writing-rust` (this repository's Rust conventions). Read both before changing commands.

## Commands

- `mise run dev -- <arguments>` runs the CLI from source, such as `mise run dev -- cli search "call a tool"`. Point `XDG_CONFIG_HOME`, `XDG_STATE_HOME` and `XDG_RUNTIME_DIR` at a temporary directory to keep experiments out of your real configuration, approvals and daemon.
- `mise run cli:reference` regenerates `crates/mcp/docs/cli.md` after a registry change; the tests fail until it matches.
- `mise run build`, `test`, `lint`, `doc`: Cargo over the workspace. `cargo test -p mcp` also builds the `mcp-test-server` example the process tests start.
- `mise run package [-- <target>]` writes `artifacts/mcp-<target>.tar.gz`, this machine's target by default.
- `mise run release:formula` writes the Homebrew formula for the archives in `artifacts/`.
- `mise run release` publishes the GitHub release through Knope; only release.yml runs it.

## Platform rules and preview

- One registry entry per command generates the parser, `--help`, `schema`, `cli search` and the reference. Never add a Clap argument, a default or an output field outside it.
- JSON on stdout regardless of terminal; one structured error on stderr with a stable exit status; no prompts; every command that changes something has `--dry-run`, which changes nothing.
- Reads never start the session daemon or a server; `Connects` commands may. Tool results pass through unchanged; everything else `mcp` reports is redacted of configured and interpolated secret values.
- No flag, variable or file approves a project server without a person. Tests seed `approvals.json`; they never run `servers approve` where it could ask.
- macOS and Linux only: the daemon uses Unix domain sockets.
- A reviewer exercises a change with the built binary in a temporary home: `export XDG_CONFIG_HOME="$(mktemp -d)" XDG_STATE_HOME="$(mktemp -d)" XDG_RUNTIME_DIR="$(mktemp -d)"`, a `.mcp.json` naming `target/debug/examples/mcp-test-server`, then discover (`cli search`), inspect (`schema`, `tools info`), preview (`--dry-run`), run (`mcp call`), and verify with a read (`servers info`, `context show`), including the failure the change affects; `mcp servers disconnect --all` stops the servers afterwards.
