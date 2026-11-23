//! The `mcp` executable: a shell around [`mcp::app::run`], and the session daemon when started
//! as `mcp __daemon <socket>`, which only `mcp` itself does.

use std::path::PathBuf;
use std::process::ExitCode;

use mcp::failure::Failure;

/// The internal mode `mcp` starts its session daemon in. It is not a command: the registry, the
/// parser, and discovery never list it.
const DAEMON_MODE: &str = "__daemon";

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    if let [_, mode, socket] = args.as_slice()
        && mode == DAEMON_MODE
    {
        return ExitCode::from(mcp::daemon::server::run(PathBuf::from(socket)));
    }
    let status = match std::env::current_dir() {
        Ok(cwd) => mcp::app::run(&args, cwd),
        Err(error) => mcp::app::fail_early(&Failure::precondition(format!(
            "cannot read the current directory: {error}"
        ))),
    };
    ExitCode::from(status)
}
