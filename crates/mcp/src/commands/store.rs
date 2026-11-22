//! `mcp store migrate`: preparing and upgrading the note store.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::commands::notes::ReceiptStatus;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{CommandSpec, DRY_RUN, Effect, Example, Requirement, STORE};
use crate::response::{RenderText, Response};
use crate::store;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "store.migrate",
    path: &["store", "migrate"],
    summary: "Create the store or upgrade its schema to this mcp's version",
    description: "Applies every schema migration the store lacks, oldest first, under the \
        store's write lock, creating the store and its directory when they do not exist. Other \
        commands refuse a store with pending migrations (PRECONDITION_FAILED), so run this after \
        upgrading mcp when they ask for it. The receipt lists the `migrations` applied; an \
        empty list means the store was already current, so running it again is safe. A store \
        written by a newer mcp, or a SQLite database another program wrote, is refused and \
        left unchanged. --dry-run lists the migrations it would apply and changes nothing.",
    arguments: &[DRY_RUN, STORE],
    effect: Effect::Modifies("the store's schema"),
    requires: &[Requirement::Store],
    dry_run: true,
    idempotent: true,
    errors: &[ErrorCode::InvalidArgument, ErrorCode::PreconditionFailed, ErrorCode::Unavailable],
    examples: &[Example {
        summary: "Show the migrations the store needs, changing nothing",
        args: &["store", "migrate", "--dry-run"],
    }],
    synonyms: &["upgrade", "schema", "database", "initialize", "init", "prepare", "setup"],
    output_schema: || schema_for!(MigrationReceipt),
    run: migrate,
}];

/// The outcome of `mcp store migrate`.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReceipt {
    pub status: ReceiptStatus,
    /// The store's absolute path.
    pub store: String,
    /// Whether the command created a new store: the file, or its schema, did not exist (for
    /// `dry-run`: would create it).
    pub store_created: bool,
    /// The migrations applied (for `dry-run`: that would be applied), oldest first; empty when
    /// the store was current.
    pub migrations: Vec<String>,
}

impl RenderText for MigrationReceipt {
    fn render_text(&self) -> String {
        let verb = match self.status {
            ReceiptStatus::Completed => "Applied",
            ReceiptStatus::DryRun => "Dry run: would apply",
        };
        if self.migrations.is_empty() {
            return format!("{} is current; nothing to apply\n", self.store);
        }
        format!("{verb} {} to {}\n", self.migrations.join(", "), self.store)
    }
}

fn migrate(invocation: &Invocation) -> Result<Response, Failure> {
    let location = invocation.store_location()?;
    let (status, store_created, migrations) = if invocation.dry_run() {
        let (created, pending) = store::pending_migrations(&location)?;
        (ReceiptStatus::DryRun, created, pending)
    } else {
        let migrated = store::migrate(&location)?;
        (ReceiptStatus::Completed, migrated.created, migrated.applied)
    };
    Ok(Response::new(&MigrationReceipt {
        status,
        store: location.display(),
        store_created,
        migrations,
    }))
}
