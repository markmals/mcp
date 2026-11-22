//! Every resource's commands. Each module declares its registry entries in `COMMANDS`.

use crate::registry::CommandSpec;

pub mod cli;
pub mod context;
pub mod docs;
pub mod notes;
pub mod schema;
pub mod store;

/// The registry, by resource, in the order discovery lists them.
pub static GROUPS: &[&[CommandSpec]] = &[
    cli::COMMANDS,
    schema::COMMANDS,
    docs::COMMANDS,
    context::COMMANDS,
    notes::COMMANDS,
    store::COMMANDS,
];
