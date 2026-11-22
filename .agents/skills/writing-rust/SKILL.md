---
name: writing-rust
description: Use when writing, reviewing, or restructuring Rust in this repository — adding a command or an argument, changing the store schema, silencing a Clippy lint, choosing an error code, deciding what goes to stdout versus stderr, handling paths that differ on Windows, or choosing between a unit test and a test that runs the binary.
---

# Writing Rust

**Write Rust that makes the correct behavior easy to express, the incorrect behavior difficult to express, and the implemented behavior easy to verify.**

`references/rust-style-guide.md` is the full house guide: API design, ownership, errors and panics, traits, concurrency, unsafe, documentation, testing, performance and dependencies. This file is only what is specific to this repository. For the command-line contract itself, read the `designing-agent-first-clis` skill.

## Layout

| Thing                                  | Where                                                                               |
| -------------------------------------- | ----------------------------------------------------------------------------------- |
| Parse, dispatch, report on the streams | `crates/mcp/src/app.rs`                                                             |
| Every command's declaration            | `COMMANDS` in `crates/mcp/src/commands/<resource>.rs`, listed in `commands::GROUPS` |
| Registry types, parser, reference      | `registry.rs`, `parser.rs`, `reference.rs`                                          |
| Domain rules and queries               | `crates/mcp/src/notes.rs`                                                           |
| Store location, opening, migrations    | `crates/mcp/src/store.rs`, `store/schema.rs`, `migrations/`                         |
| Shipped documentation                  | `crates/mcp/docs/*.md`, listed in `docs::DOCUMENTS`                                 |
| Rules about values                     | `#[cfg(test)] mod tests` beside the code                                            |
| Rules about the process                | `crates/mcp/tests/cli/` (one test binary)                                           |
| Release archives and Homebrew formula  | `xtask/` (`cargo xtask …`), never shipped                                           |

The binary is a shell around `mcp::app::run`; that is what lets the registry and parser be tested in-process.

## Adding or changing a command

1. Declare it in the resource's `COMMANDS`: path, summary, description, arguments, effect, requirements, dry-run, idempotency, errors, examples, synonyms and output schema. A new resource also goes in `registry::RESOURCES`.
2. Implement `run` with the shared pieces: `Invocation` for arguments, `Failure` for errors, `Response` for results, `RenderText` for `--output text`.
3. Run `mise run cli:reference` to regenerate `crates/mcp/docs/cli.md`.
4. `mise run test`. The registry tests fail when a parser argument is undeclared, a declared default differs at run time, an example fails or its output does not match the schema, or a modifying command lacks `--dry-run` and a dry-run example.

Never hand-write Clap definitions: the parser is generated from the registry with Clap's builder API, so a flag cannot exist without its schema, help and search entry.

## Output and errors

- Results go to stdout as JSON, whatever stdout is; `--output text` is for people. Warnings go to stderr as `warning: …` lines.
- A failure returns `Failure` with a stable `ErrorCode`; `app::report` writes `{"error": {…}}` to stderr and exits with the code's status. Add a code only when callers must branch on it, and update `ErrorCode::ALL`, its exit status and meaning.
- A closed stdout (`… | head`) is the reader's choice, not a failure; any other stdout write error is reported as `INTERNAL`.
- No `process::exit` below `main`; `main` returns `ExitCode`.

## Store changes

Add a migration directory under `crates/mcp/migrations/` (`<YYYY-MM-DD-HHMMSS>_<name>/up.sql` and `down.sql`), then update `store/schema.rs` to match. The store tests apply and revert every migration and read each column back. Existing stores are upgraded only by `mcp store migrate`; reads and dry runs must never write, so open with `store::open_for_reading` and plan changes before applying them.

## Lints

`[workspace.lints]` in the root `Cargo.toml`:

| Lint                                                  | Table    | Why it is on                                                          |
| ----------------------------------------------------- | -------- | --------------------------------------------------------------------- |
| `unreachable_pub`                                     | `rust`   | a `pub` item no public path reaches is dead or a missing `pub(crate)` |
| `undocumented_unsafe_blocks`                          | `clippy` | every `unsafe` block carries its `SAFETY:` comment                    |
| `allow_attributes_without_reason`, `allow_attributes` | `clippy` | silencing a lint is a decision with a reason, made with `#[expect]`   |
| `cast_possible_truncation`, `cast_sign_loss`          | `clippy` | casts that silently lose data; use `try_from`                         |
| `missing_errors_doc`, `missing_panics_doc`            | `clippy` | a public fallible or panicking function says when it does either      |

Silence a lint on the item, with a reason, using `#[expect]`, never `#[allow]`. `#[expect]` fails the build once the underlying problem is gone.

## Formatting

`rustfmt.toml`: 100 columns, 4 spaces, Unix newlines, `use_small_heuristics = "Max"`. Write std, then external crates, then `crate` imports as separate groups; stable rustfmt sorts within groups but cannot create them.

## Cross-platform

CI builds and tests on Linux, macOS and Windows.

- Print paths with `Path::display()`; build them with `Path::join`.
- Compare paths in tests by canonical directory: macOS reports `/var` as `/private/var`, and Windows canonical paths carry a `\\?\` prefix.
- In tests use `env!("CARGO_BIN_EXE_mcp")`, which already has Windows' `.exe`.

## Testing

Unit tests cover rules: validation, planning, ranking, paging. Process tests in `tests/cli/` cover what needs a process: JSON on stdout, nothing on stdout for failures, exit statuses, and that dry runs and reads leave the fixture byte-for-byte unchanged (`Fixture::snapshot`). Every fixture points `MCP_STORE`, `XDG_DATA_HOME` and `LOCALAPPDATA` into a temporary directory, so no test touches real data.

## Verification

```sh
mise run fmt        # dprint and rustfmt, in place
mise run lint       # cargo clippy --workspace --all-targets -- -D warnings
mise run test       # cargo test --workspace
mise run doc        # rustdoc with RUSTDOCFLAGS=-D warnings
mise run check      # the gate: test, doc, every hk step, Workbench validation
```

Never substitute a weaker command for one of these, and never report a check as passing unless it ran.

## Anti-patterns — stop if you are about to

- Add `.clone()`, `Arc`, `Mutex` or `Box` to make a borrow error go away instead of deciding the ownership.
- Add `#[allow(...)]`, or widen a lint, to make a warning go away.
- `unwrap()` or `expect()` on anything a user, the store or the file system supplied.
- Swallow an error with `.ok()`, a default value or an ignored result.
- Write to the store from a read or a `--dry-run` path.
- Document a flag, default or output field anywhere but the registry and the types it derives schemas from.
- Write a test that restates the implementation, or that asserts a constant against itself.
- Claim correctness because it compiled.
