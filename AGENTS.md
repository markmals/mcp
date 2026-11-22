# MCP CLI

This project follows the Workbench process. Workbench loads from `.workbench`, a gitignored link that `mise install` restores: when `.workbench` is missing or dangling, the harness loads no Workbench content and tasks that call `workbench` fail, so run `mise install`.

This file holds only what is true of this project. The process itself comes from the Workbench package.

## Purpose and layout

`mcp` is an agent-first command-line tool in Rust: an unfamiliar agent discovers, inspects, previews, runs and verifies every operation from the installed binary. Its example domain keeps notes in a local SQLite store (Diesel, bundled SQLite, embedded migrations).

- `crates/mcp/`: the shipped CLI. `src/registry.rs` declares the command contract; `src/commands/` holds each resource's registry entries and implementations; `src/notes.rs` and `src/store.rs` are the domain and the store; `docs/` is the Markdown compiled into the binary for `docs search` (`docs/cli.md` is generated); `migrations/` is the store schema; `tests/cli/` runs the real binary.
- `xtask/`: the unpublished release helper, `cargo xtask package` and `cargo xtask formula`.
- `.agents/skills/`: `designing-agent-first-clis` (the CLI contract) and `writing-rust` (this repository's Rust conventions). Read both before changing commands.

## Commands

- `mise run dev -- <arguments>` runs the CLI from source, such as `mise run dev -- cli search "add a note"`. Pass `--store <path>` or set `MCP_STORE` to keep experiments out of your real store.
- `mise run cli:reference` regenerates `crates/mcp/docs/cli.md` after a registry change; the tests fail until it matches.
- `mise run build`, `test`, `lint`, `doc`: Cargo over the workspace.
- `mise run package [-- <target>]` writes `artifacts/mcp-<target>.tar.gz` (`.zip` on Windows), this machine's target by default.
- `mise run release:formula` writes the Homebrew formula for the archives in `artifacts/`.
- `mise run release` publishes the GitHub release through Knope; only release.yml runs it.

## Platform rules and preview

- One registry entry per command generates the parser, `--help`, `schema`, `cli search` and the reference. Never add a Clap argument, a default or an output field outside it.
- JSON on stdout regardless of terminal; one structured error on stderr with a stable exit status; no prompts; every modifying command has `--dry-run`, which writes nothing, not even a new store.
- Reads and dry runs never write to the store. Schema changes are new migrations, applied to existing stores only by `mcp store migrate`.
- A reviewer exercises a change with the built binary against a temporary store: `export MCP_STORE="$(mktemp -d)/store.sqlite3"`, then discover (`cli search`), inspect (`schema`), preview (`--dry-run`), run, and verify with a read (`notes get`, `notes list`, `context show`), including the failure the change affects.
