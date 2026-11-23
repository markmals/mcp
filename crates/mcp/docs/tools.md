# Tools

Find a tool, inspect its contract, call it, and read its result.

```sh
mcp tools search "take a screenshot"
mcp tools info chrome-devtools take_screenshot
mcp call chrome-devtools navigate_page --url https://example.com
mcp call --output text chrome-devtools take_snapshot | rg 'Example Domain'
```

## Finding tools

`mcp tools list` pages through every usable server's tools, in server-name order and then each server's own order. Each item carries the server, the tool's name and title, a one-line `summary` (at most 200 characters; `summaryTruncated` says when it was cut), and the annotations the server declared.

`mcp tools search "<what you want to do>"` ranks tools across servers by their names, titles, descriptions and parameters, locally, and returns each match with the exact `tools info` command.

Both connect to servers, starting sessions as needed. When some servers cannot answer, the command still succeeds, with `complete: false` and each such server under `unavailable` with its error `code` and `message`. Pass `--server <name>` (repeatable) to ask only some servers.

## Inspecting a tool

`mcp tools info <server> <tool>` returns the tool as the server declared it: `description`, `inputSchema`, `outputSchema` and `annotations`, plus its `flags` and an example `mcp call` command line. Annotations such as `readOnlyHint` and `destructiveHint` are the server's own hints; `mcp` does not verify them.

## Calling a tool

```text
mcp call [--input <json>|@<file>|-] [--dry-run] [--timeout <seconds>] [--output json|text] <server> <tool> [--<property> <value>]...
```

`mcp`'s own options come **before** `<server>`. Everything after `<tool>` belongs to the tool, so a tool can have a property named like an `mcp` option. When an unknown tool flag is one of `mcp`'s options, the error suggests moving it before `<server>`.

### Flags

Each top-level property of the tool's `inputSchema` is a flag:

- The canonical flag is the property name in kebab-case: `filePath` and `file_path` are `--file-path`. The property name as written, `--filePath`, always works too. A kebab-case flag that two properties share is ambiguous and rejected; use the names as written.
- `--flag value` and `--flag=value` are equivalent.
- A property whose type is or includes `string`, or whose `enum` holds only strings, takes the text unchanged.
- `integer` and `number` take a JSON number.
- `boolean`: `--flag` alone is `true`. `--flag true`, `--flag false`, `--flag=true` and `--flag=false` are explicit; the next word is taken as the value only when it is `true` or `false`.
- `array`: each occurrence adds one item, converted by the item type with these same rules.
- Anything else (an `object`, `anyOf`, `oneOf`, or no `type`) takes JSON text.
- Repeating a flag whose property is not an array is an error.

`tools info` lists every flag with its property, type, whether it is required, its description and, for an `enum`, its accepted values. `mcp call <server> <tool> --help` prints the same as text; a property named `help` can only be set through `--input`.

### `--input`

`--input` takes a JSON object, `@path` to read one from a file, or `-` to read one from standard input; without it the input starts as `{}`. Flags set or replace top-level properties of that object, so a script can pipe a base object and override one property:

```sh
jq -n '{url: "https://example.com"}' | mcp call --input - chrome-devtools navigate_page --timeout 30
```

### Validation and `--dry-run`

The assembled input is checked against the tool's `inputSchema` before it is sent. A violation fails with `INVALID_ARGUMENT`; `details.violations` gives each one's instance path, keyword and flag, `details.missing` lists missing required properties as flags, and the suggestion is `mcp tools info <server> <tool>`. If `mcp` cannot compile the schema, it sends the input unchecked and says so in `inputValidation`.

`--dry-run` starts or reuses the session to read the tool's schema, assembles and checks the input, and returns what it would send without calling the tool. The server's own validation and the tool's effects stay unknown until it runs.

## Results

A call returns `{server, tool, session, inputValidation, result}`. `result` is the server's `CallToolResult` unchanged: `content` blocks, `structuredContent` when the tool declares an output schema, and `isError`. `--output text` prints the text content blocks, and one line for each other block naming its type, MIME type and size.

## When a call fails

| Code                  | Meaning                                                                                                                                                                                   |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TOOL_ERROR`          | The tool ran and reported an error (`isError: true`). Stdout is empty; `details.result` holds the whole `CallToolResult`.                                                                 |
| `INVALID_ARGUMENT`    | A flag or the input does not fit the tool, or the server rejected the request's parameters.                                                                                               |
| `NOT_FOUND`           | No such server, or the server has no such tool.                                                                                                                                           |
| `TIMEOUT`             | No answer within `--timeout`. The tool may have run: verify before retrying, unless the tool declares `readOnlyHint` or `idempotentHint`.                                                 |
| `SERVER_ERROR`        | The server answered with another protocol error; `details` carries its `code`, `message` and `data`.                                                                                      |
| `UNAVAILABLE`         | The server could not be started or reached, or the connection was lost. Retrying can succeed.                                                                                             |
| `PRECONDITION_FAILED` | The configuration file is invalid, the server's entry is unusable, a variable it needs is unset, or approval is required and missing. See `docs/configuration.md` and `docs/approval.md`. |
