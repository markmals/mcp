## 0.0.1 (2026-11-23)

### Features

#### Call the tools of configured MCP servers from the command line

mcp reads the MCP servers a project declares in its `.mcp.json` and those you declare in `$XDG_CONFIG_HOME/.mcp.json`, and lets any agent with a shell list servers (`mcp servers list`, `mcp servers info`), find and inspect tools (`mcp tools list`, `mcp tools search`, `mcp tools info`), and call them with their input properties as flags (`mcp call <server> <tool> --url https://example.com`). Servers run over stdio or Streamable HTTP and keep their sessions between commands in a per-user daemon. Set `MCP_REQUIRE_APPROVAL=1` to keep a project's servers from running until a person approves them with `mcp servers approve` (Touch ID, Apple Watch or the login password on macOS; polkit on Linux). Windows is not supported.
