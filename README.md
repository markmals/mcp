# MCP CLI

<!-- Replace this paragraph: what MCP CLI does, who runs it, and why an agent or a person would reach for it. -->

`mcp` is a command-line tool built for coding agents first and people second. Every operation is discoverable from the binary itself: search for a command, inspect its full contract, preview it with `--dry-run`, run it, and verify the effect. Results are JSON on stdout, errors are structured JSON on stderr with stable exit statuses, and nothing ever prompts.

## Install

With [Mise](https://mise.jdx.dev), from the GitHub release archives:

```sh
mise use github:markmals/mcp
```

With [Homebrew](https://brew.sh):

```sh
brew install markmals/tap/mcp
```

Release archives for macOS (Apple silicon and Intel), Linux (arm64 and x64, static musl builds) and Windows (x64) are attached to every [GitHub release](https://github.com/markmals/mcp/releases), each with a build attestation: `gh attestation verify <archive> --repo markmals/mcp`.

### Other programs named `mcp`

Two other projects install an executable named `mcp`: the [MCP Python SDK](https://github.com/modelcontextprotocol/python-sdk) through its `cli` extra (`mcp[cli]`), which an activated Python virtual environment can put on `PATH`, and [MCP Tools](https://github.com/f/mcptools) (`brew install f/mcptools/mcp`). When more than one is installed, the shell runs whichever comes first on `PATH`; `type -a mcp` lists them in that order. Homebrew links only one formula's `bin/mcp` at a time, so `markmals/tap/mcp` and `f/mcptools/mcp` cannot both be linked.

## Use

```sh
mcp --help                                   # orientation and the discovery commands
mcp cli search "create a note"               # find the command for a task
mcp schema notes create                      # its arguments, effects, errors, and result schema
mcp context show                             # the store commands will use
mcp notes create --name groceries --title Groceries --body "Milk, eggs" --dry-run
mcp notes create --name groceries --title Groceries --body "Milk, eggs"
mcp notes get groceries                      # verify
mcp docs search "revision conflict"          # concepts, from documentation built into the binary
```

Notes live in one SQLite file: `--store <path>`, else `MCP_STORE`, else `$XDG_DATA_HOME/mcp/store.sqlite3` (`~/.local/share/mcp/store.sqlite3`; `%LOCALAPPDATA%\mcp\store.sqlite3` on Windows). The [CLI reference](crates/mcp/docs/cli.md) documents every command; it is generated from the command registry.

## Develop

Prerequisites: [Mise](https://mise.jdx.dev) 2026.11.9 or newer. It installs Rust, Workbench, hk, Beads, dprint, Knope and the other tools `mise.toml` pins.

```sh
mise trust && mise install
mise run setup            # crates, Beads, git hooks
mise run check            # the gate CI runs: tests, rustdoc, formatting, Clippy, workflows, records
```

| Task                             | What it does                                          |
| -------------------------------- | ----------------------------------------------------- |
| `mise run dev -- <args>`         | Run the CLI from source                               |
| `mise run test`                  | Unit tests and tests that run the binary              |
| `mise run fmt`                   | Format everything (dprint, rustfmt)                   |
| `mise run lint`                  | Clippy over every target, warnings as errors          |
| `mise run cli:reference`         | Regenerate `crates/mcp/docs/cli.md` from the registry |
| `mise run package [-- <target>]` | Build a release archive into `artifacts/`             |
| `mise run status`                | Where the work stands in the Workbench process        |

## Layout

```text
crates/mcp/
  src/registry.rs       the command contract every interface is generated from
  src/commands/         each resource's commands: cli, schema, docs, context, notes, store
  src/notes.rs          the notes domain: validation, change planning, queries
  src/store.rs          store location, read-only opening, migrations
  migrations/           the SQLite schema, embedded in the binary
  docs/                 Markdown built into the binary for `docs search`
  tests/cli/            tests that run the binary
xtask/                  release helper: archives and the Homebrew formula
```

## Releasing

Every pull request carries a Knope change file (`workbench change-files create`) or a `Release-Note: none` trailer. On `main`, the Prepare release workflow keeps a release pull request open; merging it runs the gate, builds, smoke-tests and attests the five archives, runs `knope release` to tag `v<version>` and publish the GitHub release with the archives, and then pushes the Homebrew formula to `markmals/homebrew-tap` when the repository variable `HOMEBREW_TAP` is `true` (otherwise that job is skipped).

Credentials: publishing the GitHub release uses the workflow's built-in `GITHUB_TOKEN`; no stored secret is needed. The Homebrew job alone uses the `homebrew` GitHub environment, which holds `HOMEBREW_TAP_TOKEN`: create that environment under Settings → Environments, restrict its deployment branches to `main`, add a fine-grained token with contents write access to `markmals/homebrew-tap` only as its `HOMEBREW_TAP_TOKEN` secret, then set the repository variable `HOMEBREW_TAP` to `true`.
