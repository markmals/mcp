//! `mcp`: an agent-first command-line client for the Model Context Protocol servers configured
//! in a project's `.mcp.json` and the user's `$XDG_CONFIG_HOME/.mcp.json`.
//!
//! The binary is a shell around [`app::run`], and around [`daemon::server::run`] when it runs as
//! its own session daemon. Every command is declared once in the [`registry`]; the parser,
//! `--help`, `schema`, `cli search`, and the CLI reference are generated from it.

pub mod app;
pub mod approval;
pub mod commands;
pub mod config;
pub mod connect;
pub mod daemon;
pub mod docs;
pub mod failure;
pub mod invocation;
pub mod parser;
pub mod redact;
pub mod reference;
pub mod registry;
pub mod response;
pub mod search;
pub mod tool_flags;
pub mod tool_input;

/// The executable's name, as commands and suggestions spell it.
pub const NAME: &str = env!("CARGO_PKG_NAME");
