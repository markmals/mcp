//! The per-user session daemon: `mcp` itself, started as `mcp __daemon <socket>`, holding one
//! session per workspace and server so a server keeps its state across invocations.
//!
//! [`client`] is the synchronous side every command uses; [`server`] is the daemon process.

pub mod client;
pub mod paths;
pub mod protocol;
pub mod server;

mod connection;
mod dispatch;
mod failures;
mod handler;
mod http;
mod session;
mod sessions;
mod stderr_tail;
mod stdio;
