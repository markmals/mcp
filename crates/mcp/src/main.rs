//! The `mcp` executable: a shell around [`mcp::app::run`].

use std::process::ExitCode;

use mcp::failure::Failure;

fn main() -> ExitCode {
    let status = match std::env::current_dir() {
        Ok(cwd) => mcp::app::run(&std::env::args_os().collect::<Vec<_>>(), cwd),
        Err(error) => mcp::app::fail_early(&Failure::precondition(format!(
            "cannot read the current directory: {error}"
        ))),
    };
    ExitCode::from(status)
}
