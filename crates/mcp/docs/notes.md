# Notes

A note is a named piece of text in the store. `mcp notes` lists, reads, creates, updates and deletes notes; `mcp schema notes <operation>` has the exact arguments, errors and result schema of each.

## Names

A name is a note's permanent identifier: lowercase letters and digits in hyphen-separated words, at most 64 characters, such as `groceries` or `trip-2027`. `notes create --name` takes a new name; `notes get`, `notes update` and `notes delete` take an existing one as their first argument. Names cannot be changed: create a note with the new name, then delete the old one.

## Titles and bodies

A title is one line of at most 200 characters. A body is free text of at most 65536 bytes. `--body` accepts the text itself, `@<file>` to read a file, or `-` to read standard input, so a long body never needs shell escaping:

```sh
mcp notes create --name meeting --title "Weekly meeting" --body @agenda.md
printf 'Milk\nEggs\n' | mcp notes update groceries --body -
```

## Revisions and safe changes

Every note has a `revision` that starts at 1 and increases by one on every change. An update whose values equal the stored ones changes nothing: its receipt reports `action` `unchanged` and the revision stays the same, so repeating an update is safe.

Pass `--expect-revision <revision>` to `notes update` or `notes delete` to change the note only if it is still the version you read. When another process changed it in between, the command fails with `CONFLICT`, exit status 4, and `details.actualRevision`; read the note again with `mcp notes get <name>` and decide whether to retry.

## Receipts

`notes create`, `notes update` and `notes delete` print a receipt:

- `status`: `completed` when the change is written, `dry-run` when nothing was written.
- `action`: `created`, `updated`, `unchanged` or `deleted`. For a dry run, what the real run would do.
- `store`: the absolute path of the store the command used.
- `storeCreated`: whether this command created a new store. Of two commands racing to create one, exactly one reports `true`.
- `note`: the note after the change; for `deleted`, the note as it was.
- `changed`, `previousRevision`, and for a dry run the `caveats` it cannot promise.

A receipt describes what the command did. Verify the effect independently with `mcp notes get <name>` or `mcp notes list`.

## Previewing with dry run

Every modifying command accepts `--dry-run`. A dry run validates the input, resolves the store, reads the note, makes the same decision the real run would, and returns that receipt with `status` `dry-run`. It writes nothing, not even a new store. It cannot promise that another process will leave the note alone before the real run; `--expect-revision` turns that race into a `CONFLICT` instead of a lost change.

## Pagination

`mcp notes list` returns notes in name order, at most `--limit` (default 20, at most 100) at a time, as `items` with `hasMore` and `nextCursor`. When `hasMore` is true, pass `nextCursor` as `--cursor` to get the next page; repeat until `hasMore` is false. A first page is never the whole collection unless `hasMore` is false.

```sh
mcp notes list --limit 2
mcp notes list --limit 2 --cursor groceries
```

## Errors

A failing command prints nothing on stdout. It writes one JSON document to stderr, `{"error": {"code", "message", "retryable", "details", "suggestions"}}`, and exits with the code's status: `INVALID_ARGUMENT` 2, `NOT_FOUND` 3, `PRECONDITION_FAILED` 3, `CONFLICT` 4, `UNAVAILABLE` 5 (retryable), `INTERNAL` 70. `suggestions` lists commands that recover from the failure.
