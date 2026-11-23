//! Process-level tests: each one runs the real `mcp` binary, against the `mcp-test-server`
//! example where a server is needed.
//!
//! One test binary keeps link time down and lets every module share `support`.

mod approval;
mod call;
mod configuration;
mod discovery;
mod reference;
mod registry;
mod secrets;
mod servers;
mod support;
mod tools;
