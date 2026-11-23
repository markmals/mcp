# Approval

A project's `.mcp.json` names programs to run and URLs to contact. Approval makes those servers wait until a person has approved them. It is off by default: `mcp` then runs a project's servers as Claude Code's non-interactive runs do.

## Turning it on

Set `MCP_REQUIRE_APPROVAL=1` in the environment `mcp` runs in. Unset or `0` means approval is not required. Any other value makes every command that resolves servers fail with `PRECONDITION_FAILED`, naming the variable. `mcp context show` reports whether approval is required.

## What it covers

Every server from a project file, stdio and HTTP alike. Servers in the user file are the user's own and never need approval.

Each server reports its `approval` state in `servers list` and `servers info`, whether or not approval is required:

| State          | Meaning                                                         |
| -------------- | --------------------------------------------------------------- |
| `approved`     | A person approved this server's configuration as it is written. |
| `unapproved`   | No approval exists for this server in this repository.          |
| `changed`      | A person approved an earlier configuration of this server.      |
| `not-required` | A user-file server.                                             |

While approval is required, a command that would connect to an `unapproved` or `changed` project server fails with `PRECONDITION_FAILED`. Its details carry `server`, `file` and `approval`, and its suggestion is the `mcp servers approve` command a person must confirm. Multi-server commands such as `tools list` report the server under `unavailable` instead.

## Approving

```sh
mcp servers approve chrome-devtools --dry-run   # what the person would be asked to approve
mcp servers approve chrome-devtools             # asks the person, then records the approval
mcp servers approve --all                       # every project server, in one request
```

`servers approve` always asks the person, whether or not approval is currently required, so servers can be approved before it is turned on. It waits up to `--timeout` seconds (default 120) and then withdraws the request with `TIMEOUT`. A cancelled request, failed authentication, or no way to ask fails with `PERMISSION_DENIED`, says which, and records nothing. User-file servers return `not-required` without asking.

- **macOS:** the system's authentication dialog (LocalAuthentication's `deviceOwnerAuthentication` policy) accepts Touch ID, Apple Watch, or the login password. It names each server and its command or URL. It needs a graphical login session, so it cannot be answered over SSH.
- **Linux:** polkit asks through the session's authentication agent for the user's own password, or a fingerprint where the system offers one. It needs the action below installed and an authentication agent running.

There is no flag, variable or file format that approves a server without the person.

### The Linux polkit action

Install this file as `/usr/share/polkit-1/actions/io.github.markmals.mcp.policy`, owned by root and mode `0644`. `mcp` never installs it. `auth_self` asks every time; `auth_self_keep` would let a second approval pass for several minutes without the person, so do not use it.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
  "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <vendor>mcp</vendor>
  <vendor_url>https://github.com/markmals/mcp</vendor_url>
  <action id="io.github.markmals.mcp.approve-servers">
    <description>Approve a project's MCP servers</description>
    <message>Authentication is required to let mcp run this project's MCP servers</message>
    <defaults>
      <allow_any>no</allow_any>
      <allow_inactive>no</allow_inactive>
      <allow_active>auth_self</allow_active>
    </defaults>
  </action>
</policyconfig>
```

## What an approval binds

- **The repository:** the git common directory, so every worktree of a clone shares approvals and another clone does not. Outside git, the directory holding the project file.
- **The project file's path** relative to the worktree root.
- **The server's name.**
- **Its fingerprint:** SHA-256 of the server's entry as written, with keys sorted and insignificant whitespace removed. Changing a command, an argument, a URL, a literal value or a `${VAR}` reference changes the fingerprint; changing the value a `${VAR}` holds does not.

A server can have several approved fingerprints, so switching between branches that configure it differently does not ask again. `servers info` reports a server's fingerprint.

## Revoking

```sh
mcp servers revoke chrome-devtools --dry-run
mcp servers revoke --all
```

Revoking removes every approval of the server in this repository and needs no confirmation. While approval is required, it also ends the server's sessions in this workspace.

## Where approvals are kept

`$XDG_STATE_HOME/mcp/approvals.json` when `XDG_STATE_HOME` is an absolute path, else `~/.local/state/mcp/approvals.json`, mode `0600`. Each record holds the binding, the command or URL as written, and when it was approved. It never holds `env` or `headers` values.

## What approval protects against

Running `mcp` in a repository and unknowingly starting the programs or contacting the URLs that repository configured, including when an agent follows an error's suggestion without reading it. It does not protect against a process that edits `mcp`'s state files, changes `mcp`'s environment, or runs the configured command itself: such a process already has the access the server would have.
