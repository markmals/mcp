---
name: designing-agent-first-clis
description: Use when designing, implementing, or reviewing command-line tools for coding agents, especially command discovery, machine-readable contracts, unattended mutations, or verification of effects.
---

# Designing Agent-First CLIs

An unfamiliar agent must discover, inspect, preview, execute and verify an operation using the installed binary. A skill supplements that interface; it does not replace it.

## Define one contract

Use one authoritative registry or annotated native command definition. Generate the parser, help, discovery metadata, schemas and reference from it. Include arguments/defaults, constraints, output schemas, effects, prerequisites, errors, dry-run/idempotency semantics and executable examples. Parser metadata alone is insufficient for effects and result contracts. Avoid hand-synchronized command tables.

Use a consistent resource/operation grammar, kebab-case flags and stable identifiers. Add domain operations rather than forcing everything into CRUD.

## Discovery surface

| Command                      | Result                                                                   |
| ---------------------------- | ------------------------------------------------------------------------ |
| `tool --help`                | Brief orientation and discovery entry points, not the complete catalogue |
| `tool cli search "intent"`   | Bounded, ranked candidates with exact commands and effects               |
| `tool schema <command>`      | Complete selected-operation contract                                     |
| `tool docs search "concept"` | Documentation locations and relevant excerpts                            |
| `tool context show`          | Effective project, account, environment and target                       |
| `tool --version`             | Installed implementation version                                         |

Include all six discovery entry points, even for a small CLI; `docs search` can search shipped Markdown locally. Discovery is read-only and credential-free where possible. Report bounded results explicitly; never silently present a truncated collection as complete.

## Wire contract

JSON is the default on terminals and pipes alike; use explicit `--output text` for people. Stdout contains results only. Stderr carries diagnostics and structured errors; keep tokens, progress and decoration out of result JSON.

Errors contain stable `code`, `message`, `retryable`, useful `details` and reliable recovery suggestions. Failures exit nonzero. Example output must obey the same schema as runtime output, including validation and dry-run failures.

Collections expose pagination. Mutation receipts identify target, resource and actual outcome. Distinguish queued/accepted, completed, partial and failed states. Submission may succeed before completion; status reads may report unfinished work successfully. An explicit wait for completion exits nonzero on timeout and reports the last known state without claiming the remote operation failed. Provide independent status/read-back.

## Mutation safety

`--dry-run` validates inputs and resolves the target without any writes, provisioning, enqueueing or other mutation—not just without the final action. State what it cannot predict. Bind dangerous actions to explicit authorization; missing TTY is never consent.

Missing required input fails promptly with repair guidance. Never silently select production. Keep credentials out of arguments, plans and output. Make retries/idempotency explicit; verify effects independently instead of treating exit zero as proof. Inspect partial receipts even when an explicitly documented local success exits zero.

## Example flow

Illustrative contract for a new storage CLI, not commands to run blindly:

```sh
store cli search "create a test database"
store schema databases create
store context show
store databases create --target local --name sample --dry-run
store databases create --target local --name sample
store databases get sample --target local
```

## Verification and common mistakes

Exercise registered examples and schema/default parity. Test missing input, empty/paginated results, authorization, dry-run side effects, stale targets, partial work, cancellation and asynchronous completion. Run fresh agents from only top-level help; measure correct end state, invalid commands, verification and unsafe effects.

Common failures: TTY-dependent JSON; a full command-tree dump instead of search; plain-text error/dry-run examples; success that only means queued; a quick-reference skill substituted for live discovery.

Derived from Mark's _Designing Agent-First Command-Line Interfaces_. Public background: [Cloudflare's CLI design](https://blog.cloudflare.com/cloudflare-cf-cli-launch/).
