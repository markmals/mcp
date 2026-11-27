# MCP CLI

`mcp` calls the tools of [Model Context Protocol](https://modelcontextprotocol.io) servers from the command line. It reads the servers a project declares in its `.mcp.json`, the file Claude Code, VS Code and other clients already read, and the servers you declare once in `$XDG_CONFIG_HOME/.mcp.json`, then lets any coding agent with a shell find, inspect and call their tools, pipe the results through `jq`, and script them, with no MCP support in its own harness and no tool catalogue loaded into its context.

It is built for coding agents first and people second. Every operation is discoverable from the binary itself: search for a command, inspect its full contract, preview it with `--dry-run`, run it, and verify the effect. Results are JSON on stdout, errors are structured JSON on stderr with stable exit statuses, and nothing prompts on the agent's side; only `mcp servers approve` asks a person, through the system's own authentication dialog.

## Install

With [Mise](https://mise.jdx.dev), from the GitHub release archives:

```sh
mise use github:markmals/mcp
```

With [Homebrew](https://brew.sh):

```sh
brew install markmals/tap/mcp
```

Release archives for macOS (Apple silicon and Intel) and Linux (arm64 and x64, static musl builds) are attached to every [GitHub release](https://github.com/markmals/mcp/releases), each with a build attestation: `gh attestation verify <archive> --repo markmals/mcp`. Windows is not supported: the session daemon uses Unix domain sockets.

### Other programs named `mcp`

Two other projects install an executable named `mcp`: the [MCP Python SDK](https://github.com/modelcontextprotocol/python-sdk) through its `cli` extra (`mcp[cli]`), which an activated Python virtual environment can put on `PATH`, and [MCP Tools](https://github.com/f/mcptools) (`brew install f/mcptools/mcp`). When more than one is installed, the shell runs whichever comes first on `PATH`; `type -a mcp` lists them in that order. Homebrew links only one formula's `bin/mcp` at a time, so `markmals/tap/mcp` and `f/mcptools/mcp` cannot both be linked.

## Use

```sh
mcp --help                                     # orientation and the discovery commands
mcp cli search "call a tool"                   # find the command for a task
mcp context show                               # the configuration files, workspace, and daemon in effect
mcp servers list                               # the configured servers and whether each is usable
mcp tools search "take a screenshot"           # tools across servers, each with the command to inspect it
mcp tools info chrome-devtools navigate_page   # its description, schemas, and flags
mcp call chrome-devtools navigate_page --url https://example.com
mcp call --output text chrome-devtools take_snapshot | rg 'Example Domain'
mcp docs search "approval"                     # concepts, from documentation built into the binary
```

A tool's input properties are flags after the tool's name (`filePath` is `--file-path`); `mcp`'s own options go before the server. `--input` takes the whole input as JSON, from a file (`@input.json`), or from stdin (`-`). Server processes live in a per-user session daemon, so a server starts once and keeps its state, such as a selected browser page, between commands.

### Configuration

```json
{
    "mcpServers": {
        "chrome-devtools": { "command": "npx", "args": ["-y", "chrome-devtools-mcp@latest"] },
        "figma": { "type": "http", "url": "http://127.0.0.1:3845/mcp" }
    }
}
```

Put personal servers in `$XDG_CONFIG_HOME/.mcp.json` (`~/.config/.mcp.json`); they apply in every directory. A project's nearest `.mcp.json`, searched from the working directory up to the git worktree root, adds its own servers and replaces a personal server of the same name. `mcp` supports `command`, `args`, `env`, `url`, `type` and `headers`, expands `${VAR}` and `${VAR:-default}`, and refuses any field it does not implement rather than ignoring it. Servers that need OAuth sign-in are not supported yet. [`docs/configuration.md`](crates/mcp/docs/configuration.md) is the full contract.

### Approval

A project's `.mcp.json` names programs to run. Set `MCP_REQUIRE_APPROVAL=1` to keep a project's servers from running until a person approves them with `mcp servers approve <name>`: Touch ID, Apple Watch or the login password on macOS, polkit on Linux. An approval is bound to the repository and to the server's configuration as written, so changing a command or an argument needs a new one. [`docs/approval.md`](crates/mcp/docs/approval.md) describes it, including the polkit action Linux needs.

The [CLI reference](crates/mcp/docs/cli.md) documents every command; it is generated from the command registry.

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
  src/commands/         each resource's commands: cli, schema, docs, context, servers, tools, call
  src/config/           the configuration files: discovery, validation, interpolation, precedence
  src/approval/         approvals: the state file and asking the person (LocalAuthentication, polkit)
  src/daemon/           the session daemon and its client: sessions over stdio and Streamable HTTP
  src/connect.rs        reaching a configured server: approval gate, interpolation, redaction
  src/tool_flags.rs     a tool's input properties as command-line flags
  docs/                 Markdown built into the binary for `docs search`
  examples/mcp-test-server/   the MCP server the tests run against; never shipped
  tests/cli/            tests that run the binary
xtask/                  release helper: per-target archives
```

## Releasing

Every pull request carries a Knope change file (`workbench change-files create`) or a `Release-Note: none` trailer. On `main`, the Prepare release workflow keeps a release pull request open. That needs Settings → Actions → General → "Allow GitHub Actions to create and approve pull requests". Merging the release pull request runs the gate, then builds, smoke-tests and attests the four archives. It then runs `knope release` to tag `v<version>` and publish the GitHub release with the archives, which Mise's `github:` backend installs. GitHub creates build attestations only for public repositories.

Change files choose the next version. To choose it yourself, set the repository variable `RELEASE_VERSION` (`gh variable set RELEASE_VERSION --body 1.0.0`). The release pull request uses that version while it is above the version on `main`, and the variable stops applying once a release reaches it. It is set to `0.1.0`: 0.0.1 was published early, and the next release is 0.1.0.

Homebrew builds `mcp` from source in [`markmals/homebrew-tap`](https://github.com/markmals/homebrew-tap) and serves bottles. After a release, open a pull request in the tap named `mcp-<version>` that moves `Formula/mcp.rb` to the new tag: its `tag` and the `revision` that tag points at (`git rev-parse 'v<version>^{commit}'`). The tap's `brew test-bot` workflow builds and tests bottles on that pull request. Once it passes, label the pull request `pr-pull`, and the tap's publish workflow uploads the bottles and lands the formula.

Credentials: publishing the GitHub release uses the workflow's built-in `GITHUB_TOKEN`; no stored secret is needed.

## License

MIT. See [LICENSE](LICENSE).
