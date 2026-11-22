//! `mcp context show`: where commands take effect.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::NAME;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{CommandSpec, Effect, Example, Requirement, STORE};
use crate::response::{RenderText, Response};
use crate::store::{self, Inspection, STORE_ENVIRONMENT_VARIABLE, StoreSource};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "context.show",
    path: &["context", "show"],
    summary: "Show the store commands read and write, where its path came from, and its state",
    description: "Resolves the store exactly as every other command does (`--store`, else \
        MCP_STORE, else the default data location) and reports its absolute `path`, the \
        `source` of that path, and its `state`: `absent` (the first change creates it), `ready` \
        (with the number of `notes`), `needs-migration` (with `pendingMigrations`; run \
        `mcp store migrate`), or `unusable` (with the `problem`). Never creates or changes \
        the store, and reports an unusable store instead of failing.",
    arguments: &[STORE],
    effect: Effect::Reads,
    requires: &[Requirement::Store],
    dry_run: false,
    idempotent: true,
    errors: &[ErrorCode::InvalidArgument, ErrorCode::PreconditionFailed, ErrorCode::Unavailable],
    examples: &[Example { summary: "Show the store in effect", args: &["context", "show"] }],
    synonyms: &[
        "where",
        "location",
        "path",
        "data",
        "database",
        "file",
        "environment",
        "config",
        "target",
        "status",
    ],
    output_schema: || schema_for!(Context),
    run: show_context,
}];

/// Where commands take effect.
#[derive(Serialize, JsonSchema)]
pub struct Context {
    /// The mcp version answering.
    pub version: String,
    pub store: StoreContext,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoreContext {
    /// The store's absolute path.
    pub path: String,
    /// Where the path came from: `flag` (--store), `environment` (MCP_STORE), or `default`.
    pub source: StoreSource,
    /// The environment variable that selects the store when --store is absent.
    pub environment_variable: String,
    pub state: StoreState,
    /// How many notes a `ready` store holds; null otherwise.
    pub notes: Option<i64>,
    /// Migrations `mcp store migrate` would apply; empty unless `needs-migration`.
    pub pending_migrations: Vec<String>,
    /// Why an `unusable` store cannot be used; null otherwise.
    pub problem: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum StoreState {
    /// No store yet; the first change creates it.
    Absent,
    Ready,
    /// `mcp store migrate` must upgrade it before other commands use it.
    NeedsMigration,
    Unusable,
}

impl RenderText for Context {
    fn render_text(&self) -> String {
        let store = &self.store;
        let source = match store.source {
            StoreSource::Flag => "--store".to_string(),
            StoreSource::Environment => store.environment_variable.clone(),
            StoreSource::Default => "the default location".to_string(),
        };
        let state = match store.state {
            StoreState::Absent => "absent; the first change creates it".to_string(),
            StoreState::Ready => format!("ready, {} notes", store.notes.unwrap_or(0)),
            StoreState::NeedsMigration => {
                format!("needs `{NAME} store migrate` ({})", store.pending_migrations.join(", "))
            }
            StoreState::Unusable => {
                format!("unusable: {}", store.problem.as_deref().unwrap_or("unknown"))
            }
        };
        format!("{NAME} {}\nStore: {}\n  from {source}\n  {state}\n", self.version, store.path)
    }
}

fn show_context(invocation: &Invocation) -> Result<Response, Failure> {
    let location = invocation.store_location()?;
    let inspection = store::inspect(&location)?;
    let (state, notes, pending_migrations, problem) = match inspection {
        Inspection::Absent => (StoreState::Absent, None, Vec::new(), None),
        Inspection::Ready { notes } => (StoreState::Ready, Some(notes), Vec::new(), None),
        Inspection::NeedsMigration { pending } => (StoreState::NeedsMigration, None, pending, None),
        Inspection::Unusable { problem } => (StoreState::Unusable, None, Vec::new(), Some(problem)),
    };
    Ok(Response::new(&Context {
        version: env!("CARGO_PKG_VERSION").to_string(),
        store: StoreContext {
            path: location.display(),
            source: location.source,
            environment_variable: STORE_ENVIRONMENT_VARIABLE.to_string(),
            state,
            notes,
            pending_migrations,
            problem,
        },
    }))
}
