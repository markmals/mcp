//! Process-level tests: each one runs the real `mcp` binary.
//!
//! One test binary keeps link time down and lets every module share `support`.

mod discovery;
mod notes;
mod reference;
mod registry;
mod store;
mod support;
