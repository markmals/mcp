//! `mcp notes`: the example domain's list, get, create, update, and delete.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use serde_json::json;

use crate::NAME;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::notes::{self, Change, Note};
use crate::registry::{
    ArgumentKind, ArgumentSpec, CommandSpec, DRY_RUN, Effect, Example, Requirement, STORE,
    ValueKind,
};
use crate::response::{RenderText, Response};
use crate::search::Page;
use crate::store::{self, StoreLocation};

const NAME_ARGUMENT: ArgumentSpec = ArgumentSpec {
    name: "name",
    kind: ArgumentKind::Positional,
    value: ValueKind::Name,
    required: true,
    default: None,
    multiple: false,
    summary: "The note's name",
};

const EXPECT_REVISION: ArgumentSpec = ArgumentSpec {
    name: "expect-revision",
    kind: ArgumentKind::Option,
    value: ValueKind::Integer { min: 1, max: None },
    required: false,
    default: None,
    multiple: false,
    summary: "Fail with CONFLICT unless the note is at this revision, so a change never \
              overwrites one you have not read",
};

const MUTATION_ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidArgument,
    ErrorCode::NotFound,
    ErrorCode::Conflict,
    ErrorCode::PreconditionFailed,
    ErrorCode::Unavailable,
];

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "notes.list",
        path: &["notes", "list"],
        summary: "List notes in name order, one page at a time",
        description: "Returns up to --limit notes in name order as `items`, with `hasMore` and \
            `nextCursor`; pass `nextCursor` back as --cursor for the next page, until `hasMore` \
            is false. A store that does not exist yet lists no notes and says so on stderr. Never \
            changes anything.",
        arguments: &[
            ArgumentSpec {
                name: "limit",
                kind: ArgumentKind::Option,
                value: ValueKind::Integer { min: 1, max: Some(100) },
                required: false,
                default: Some("20"),
                multiple: false,
                summary: "The most notes to return",
            },
            ArgumentSpec {
                name: "cursor",
                kind: ArgumentKind::Option,
                value: ValueKind::Name,
                required: false,
                default: None,
                multiple: false,
                summary: "Resume after a previous page: the nextCursor it returned",
            },
            STORE,
        ],
        effect: Effect::Reads,
        requires: &[Requirement::Store],
        dry_run: false,
        idempotent: true,
        errors: &[
            ErrorCode::InvalidArgument,
            ErrorCode::PreconditionFailed,
            ErrorCode::Unavailable,
        ],
        examples: &[
            Example { summary: "List the first page of notes", args: &["notes", "list"] },
            Example {
                summary: "List one note at a time",
                args: &["notes", "list", "--limit", "1"],
            },
        ],
        synonyms: &["all", "every", "show", "browse", "enumerate", "page", "index"],
        output_schema: || schema_for!(Page<Note>),
        run: list,
    },
    CommandSpec {
        id: "notes.get",
        path: &["notes", "get"],
        summary: "Read one note by name",
        description: "Returns the note with its `revision` and timestamps. Use it to verify a \
            change, or to learn the revision to pass to --expect-revision. Never changes anything.",
        arguments: &[NAME_ARGUMENT, STORE],
        effect: Effect::Reads,
        requires: &[Requirement::Store],
        dry_run: false,
        idempotent: true,
        errors: &[
            ErrorCode::InvalidArgument,
            ErrorCode::NotFound,
            ErrorCode::PreconditionFailed,
            ErrorCode::Unavailable,
        ],
        examples: &[Example {
            summary: "Read the note `groceries`",
            args: &["notes", "get", "groceries"],
        }],
        synonyms: &["read", "show", "view", "fetch", "open", "verify", "inspect", "revision"],
        output_schema: || schema_for!(Note),
        run: get,
    },
    CommandSpec {
        id: "notes.create",
        path: &["notes", "create"],
        summary: "Create a note with a new name",
        description: "Creates the note at revision 1. The name must be unused: an existing name \
            fails with CONFLICT and its revision in `details`; change that note with `mcp \
            notes update` instead. Creates the store, with its directory, when it does not exist \
            yet. The receipt reports `status` (`completed`, or `dry-run` when nothing was \
            written), `action` `created`, the `store`, whether the store was created, and the \
            `note` as stored. --dry-run validates the input and checks the name against the \
            store, and creates nothing, not even the store.",
        arguments: &[
            ArgumentSpec {
                name: "name",
                kind: ArgumentKind::Option,
                value: ValueKind::Name,
                required: true,
                default: None,
                multiple: false,
                summary: "The new note's name, its permanent identifier",
            },
            ArgumentSpec {
                name: "title",
                kind: ArgumentKind::Option,
                value: ValueKind::String,
                required: true,
                default: None,
                multiple: false,
                summary: "One line, at most 200 characters",
            },
            ArgumentSpec {
                name: "body",
                kind: ArgumentKind::Option,
                value: ValueKind::Text,
                required: false,
                default: Some(""),
                multiple: false,
                summary: "The text: the text itself, `@<file>` to read a file, or `-` to read \
                          stdin; at most 65536 bytes",
            },
            DRY_RUN,
            STORE,
        ],
        effect: Effect::Modifies("one new note in the store"),
        requires: &[Requirement::Store],
        dry_run: true,
        idempotent: false,
        errors: &[
            ErrorCode::InvalidArgument,
            ErrorCode::Conflict,
            ErrorCode::PreconditionFailed,
            ErrorCode::Unavailable,
        ],
        examples: &[
            Example {
                summary: "Preview creating a note, writing nothing",
                args: &[
                    "notes",
                    "create",
                    "--name",
                    "packing-list",
                    "--title",
                    "Packing list",
                    "--body",
                    "Passport, charger",
                    "--dry-run",
                ],
            },
            Example {
                summary: "Create the note",
                args: &[
                    "notes",
                    "create",
                    "--name",
                    "packing-list",
                    "--title",
                    "Packing list",
                    "--body",
                    "Passport, charger",
                ],
            },
        ],
        synonyms: &["add", "new", "write", "save", "make", "record", "remember", "jot"],
        output_schema: || schema_for!(NoteReceipt),
        run: create,
    },
    CommandSpec {
        id: "notes.update",
        path: &["notes", "update"],
        summary: "Change a note's title or body",
        description: "Sets the given fields and leaves the others as stored; pass at least one of \
            --title and --body. A change increments the note's `revision`; values equal to the \
            stored ones change nothing and report `action` `unchanged`, so repeating an update is \
            safe. Pass --expect-revision with the revision you read to fail with CONFLICT instead \
            of overwriting someone else's change. --dry-run reports the same receipt without \
            writing.",
        arguments: &[
            NAME_ARGUMENT,
            ArgumentSpec {
                name: "title",
                kind: ArgumentKind::Option,
                value: ValueKind::String,
                required: false,
                default: None,
                multiple: false,
                summary: "The new title: one line, at most 200 characters",
            },
            ArgumentSpec {
                name: "body",
                kind: ArgumentKind::Option,
                value: ValueKind::Text,
                required: false,
                default: None,
                multiple: false,
                summary: "The new text: the text itself, `@<file>` to read a file, or `-` to read \
                          stdin; at most 65536 bytes",
            },
            EXPECT_REVISION,
            DRY_RUN,
            STORE,
        ],
        effect: Effect::Modifies("one existing note in the store"),
        requires: &[Requirement::Store],
        dry_run: true,
        idempotent: true,
        errors: MUTATION_ERRORS,
        examples: &[
            Example {
                summary: "Preview a new title, writing nothing",
                args: &[
                    "notes",
                    "update",
                    "groceries",
                    "--title",
                    "Groceries for Sunday",
                    "--dry-run",
                ],
            },
            Example {
                summary: "Retitle the note only if nobody changed it since revision 1",
                args: &[
                    "notes",
                    "update",
                    "groceries",
                    "--title",
                    "Groceries for Sunday",
                    "--expect-revision",
                    "1",
                ],
            },
        ],
        synonyms: &["edit", "change", "modify", "rename", "retitle", "rewrite", "set", "replace"],
        output_schema: || schema_for!(NoteReceipt),
        run: update,
    },
    CommandSpec {
        id: "notes.delete",
        path: &["notes", "delete"],
        summary: "Delete a note permanently",
        description: "Removes the note; the receipt carries the note as it was, which is the only \
            copy left. Pass --expect-revision with the revision you read to fail with CONFLICT \
            instead of deleting a note someone changed. Deleting a missing note fails with \
            NOT_FOUND. --dry-run reports the same receipt without deleting.",
        arguments: &[NAME_ARGUMENT, EXPECT_REVISION, DRY_RUN, STORE],
        effect: Effect::Modifies("one existing note in the store, permanently"),
        requires: &[Requirement::Store],
        dry_run: true,
        idempotent: false,
        errors: MUTATION_ERRORS,
        examples: &[
            Example {
                summary: "Preview deleting a note",
                args: &["notes", "delete", "standup", "--dry-run"],
            },
            Example {
                summary: "Delete the note only if it is still at revision 1",
                args: &["notes", "delete", "standup", "--expect-revision", "1"],
            },
        ],
        synonyms: &["remove", "erase", "drop", "destroy", "discard", "forget", "rm"],
        output_schema: || schema_for!(NoteReceipt),
        run: delete,
    },
];

/// Whether a modifying command wrote its change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiptStatus {
    /// The change is written.
    Completed,
    /// Nothing was written; the receipt shows what running without --dry-run would do.
    DryRun,
}

/// What the change did to the note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Created,
    Updated,
    /// The requested values equal the stored ones; nothing was written.
    Unchanged,
    Deleted,
}

/// The outcome of a modifying `notes` command.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteReceipt {
    pub status: ReceiptStatus,
    /// For `dry-run`, what running without --dry-run would do.
    pub action: Action,
    /// The store's absolute path.
    pub store: String,
    /// Whether the command created a new store: the file, or its schema, did not exist (for
    /// `dry-run`: would create it).
    pub store_created: bool,
    /// The note after the change; for `deleted`, the note as it was.
    pub note: Note,
    /// The fields the change sets, such as `title`; empty for `unchanged`.
    pub changed: Vec<String>,
    /// The note's revision before the change; null for `created`.
    pub previous_revision: Option<i64>,
    /// What a dry run cannot promise about the real run; empty when `completed`.
    pub caveats: Vec<String>,
}

impl RenderText for NoteReceipt {
    fn render_text(&self) -> String {
        let action = match self.action {
            Action::Created => "created",
            Action::Updated => "updated",
            Action::Unchanged => "left unchanged",
            Action::Deleted => "deleted",
        };
        let mut text = match self.status {
            ReceiptStatus::Completed => format!(
                "Note {} {action} (revision {}) in {}\n",
                self.note.name, self.note.revision, self.store
            ),
            ReceiptStatus::DryRun => format!(
                "Dry run: note {} would be {action} (revision {}) in {}; nothing was written\n",
                self.note.name, self.note.revision, self.store
            ),
        };
        for caveat in &self.caveats {
            text.push_str(&format!("  {caveat}\n"));
        }
        text
    }
}

impl RenderText for Note {
    fn render_text(&self) -> String {
        let mut text = format!(
            "{}  {}\nrevision {}, updated {}\n",
            self.name, self.title, self.revision, self.updated_at
        );
        if !self.body.is_empty() {
            text.push('\n');
            text.push_str(&self.body);
            text.push('\n');
        }
        text
    }
}

impl RenderText for Page<Note> {
    fn render_text(&self) -> String {
        if self.items.is_empty() {
            return "No notes.".into();
        }
        let width = self.items.iter().map(|note| note.name.len()).max().unwrap_or(0);
        let mut text = String::new();
        for note in &self.items {
            text.push_str(&format!("{:width$}  {}\n", note.name, note.title));
        }
        if let Some(cursor) = &self.next_cursor {
            text.push_str(&format!("More notes follow; pass --cursor {cursor}.\n"));
        }
        text
    }
}

fn list(invocation: &Invocation) -> Result<Response, Failure> {
    let location = invocation.store_location()?;
    let limit = i64::try_from(invocation.integer("limit").unwrap_or(20)).unwrap_or(100);
    let cursor = invocation.string("cursor");
    let Some(mut connection) = store::open_for_reading(&location)? else {
        let page: Page<Note> = Page { items: Vec::new(), has_more: false, next_cursor: None };
        return Ok(Response::new(&page).warn(absent(&location)));
    };
    let (items, has_more) = notes::page(&mut connection, cursor.as_deref(), limit)?;
    let next_cursor = if has_more { items.last().map(|note| note.name.clone()) } else { None };
    Ok(Response::new(&Page { items, has_more, next_cursor }))
}

fn get(invocation: &Invocation) -> Result<Response, Failure> {
    let location = invocation.store_location()?;
    let name = required(invocation, "name");
    let note = match store::open_for_reading(&location)? {
        Some(mut connection) => notes::find(&mut connection, &name)?,
        None => None,
    };
    let note = note.ok_or_else(|| {
        let mut failure = notes::missing(&name);
        failure.details["store"] = json!(location.display());
        failure
    })?;
    Ok(Response::new(&note))
}

fn create(invocation: &Invocation) -> Result<Response, Failure> {
    let name = required(invocation, "name");
    let title = required(invocation, "title");
    notes::validate_title(&title)?;
    let body = invocation.text("body")?.unwrap_or_default();
    notes::validate_body(&body)?;
    let now = notes::now();
    let caveats = [
        "Another process can create a note with this name first; the real run then fails with \
         CONFLICT.",
        "createdAt and updatedAt are set when the real run happens.",
    ];
    mutate(
        invocation,
        &caveats,
        |existing| notes::plan_create(existing, &name, title.clone(), body.clone(), &now),
        &name,
    )
}

fn update(invocation: &Invocation) -> Result<Response, Failure> {
    let name = required(invocation, "name");
    let title = invocation.string("title");
    if let Some(title) = &title {
        notes::validate_title(title)?;
    }
    let body = invocation.text("body")?;
    if let Some(body) = &body {
        notes::validate_body(body)?;
    }
    if title.is_none() && body.is_none() {
        return Err(Failure::invalid_argument("pass at least one of --title and --body")
            .with_details(json!({ "arguments": ["title", "body"] }))
            .suggest(format!("{NAME} schema notes update")));
    }
    let expected = invocation.integer("expect-revision");
    let now = notes::now();
    let caveats = [
        "Another process can change the note before the real run; pass --expect-revision to \
         refuse a note that changed.",
        "updatedAt is set when the real run happens.",
    ];
    mutate(
        invocation,
        &caveats,
        |existing| notes::plan_update(existing, &name, title.clone(), body.clone(), expected, &now),
        &name,
    )
}

fn delete(invocation: &Invocation) -> Result<Response, Failure> {
    let name = required(invocation, "name");
    let expected = invocation.integer("expect-revision");
    let caveats = ["Another process can change or delete the note before the real run; pass \
         --expect-revision to refuse a note that changed."];
    mutate(invocation, &caveats, |existing| notes::plan_delete(existing, &name, expected), &name)
}

/// Plans the change from the stored note, then returns it (`--dry-run`) or writes it in the
/// same write transaction that read the note.
fn mutate(
    invocation: &Invocation,
    caveats: &[&str],
    plan: impl Fn(Option<Note>) -> Result<Change, Failure>,
    name: &str,
) -> Result<Response, Failure> {
    let location = invocation.store_location()?;
    if invocation.dry_run() {
        let (existing, store_created) = match store::open_for_reading(&location)? {
            Some(mut connection) => (notes::find(&mut connection, name)?, false),
            None => (None, true),
        };
        let change = plan(existing)?;
        let caveats = caveats.iter().map(|caveat| (*caveat).to_string()).collect();
        return Ok(Response::new(&receipt(
            ReceiptStatus::DryRun,
            &location,
            store_created,
            change,
            caveats,
        )));
    }
    let (mut connection, store_created) = match store::open_for_writing(&location)? {
        Some(connection) => (connection, false),
        // A missing store holds no notes. Planning against none first fails update and delete
        // with NOT_FOUND, as their dry runs do, before anything creates the store.
        None => {
            plan(None)?;
            let migrated = store::migrate(&location)?;
            (migrated.connection, migrated.created)
        }
    };
    let change = connection.immediate_transaction(|connection| {
        let change = plan(notes::find(connection, name)?)?;
        notes::apply(connection, &change)?;
        Ok::<Change, Failure>(change)
    })?;
    Ok(Response::new(&receipt(
        ReceiptStatus::Completed,
        &location,
        store_created,
        change,
        Vec::new(),
    )))
}

fn receipt(
    status: ReceiptStatus,
    location: &StoreLocation,
    store_created: bool,
    change: Change,
    caveats: Vec<String>,
) -> NoteReceipt {
    let (action, note, changed, previous_revision) = match change {
        Change::Create(note) => (Action::Created, note, vec!["title", "body"], None),
        Change::Update { before, after, changed } => {
            (Action::Updated, after, changed, Some(before.revision))
        }
        Change::Unchanged(note) => {
            let revision = note.revision;
            (Action::Unchanged, note, Vec::new(), Some(revision))
        }
        Change::Delete(note) => {
            let revision = note.revision;
            (Action::Deleted, note, Vec::new(), Some(revision))
        }
    };
    NoteReceipt {
        status,
        action,
        store: location.display(),
        store_created,
        note,
        changed: changed.into_iter().map(str::to_string).collect(),
        previous_revision,
        caveats,
    }
}

/// A required argument, which the parser guarantees is present.
fn required(invocation: &Invocation, name: &str) -> String {
    invocation.string(name).unwrap_or_default()
}

fn absent(location: &StoreLocation) -> String {
    format!("no store at {} yet; the first `{NAME} notes create` creates it", location.display())
}
