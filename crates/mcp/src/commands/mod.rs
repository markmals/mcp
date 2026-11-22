//! Every resource's commands. Each module declares its registry entries in `COMMANDS`.

use crate::registry::CommandSpec;

pub mod approval;
pub mod call;
pub mod cli;
pub mod context;
pub mod docs;
pub mod schema;
pub mod servers;
pub mod sessions;
pub mod tools;

/// The registry, by resource, in the order discovery lists them.
pub static GROUPS: &[&[CommandSpec]] = &[
    cli::COMMANDS,
    schema::COMMANDS,
    docs::COMMANDS,
    context::COMMANDS,
    servers::COMMANDS,
    approval::COMMANDS,
    sessions::COMMANDS,
    tools::COMMANDS,
    call::COMMANDS,
];
