//! MCP CLI: an agent-first command-line tool that keeps notes in a local SQLite store.
//!
//! The binary is a shell around [`app::run`]. Every command is declared once in the
//! [`registry`]; the parser, `--help`, `schema`, `cli search`, and the CLI reference are generated
//! from it.

pub mod app;
pub mod commands;
pub mod docs;
pub mod failure;
pub mod invocation;
pub mod notes;
pub mod parser;
pub mod reference;
pub mod registry;
pub mod response;
pub mod search;
pub mod store;

/// The executable's name, as commands and suggestions spell it.
pub const NAME: &str = env!("CARGO_PKG_NAME");
