//! Starting and stopping a stdio server: a child process of the daemon speaking MCP on its
//! standard input and output.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rmcp::service::{RoleClient, RunningService, serve_client};
use rustix::process::{Pid, Signal};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use super::failures::{start_failure, with_stderr};
use super::handler::Client;
use super::stderr_tail::SharedTail;
use crate::failure::Failure;

/// How long a server gets to exit after its standard input closes, and again after `SIGTERM`.
const EXIT_GRACE: Duration = Duration::from_secs(2);
/// How long a server that failed to start gets to exit and flush its stderr before it is killed.
const FAILURE_GRACE: Duration = Duration::from_millis(500);

/// What a stdio server is started with.
pub(crate) struct Launch<'a> {
    pub(crate) server: &'a str,
    pub(crate) workspace: &'a str,
    pub(crate) command: &'a str,
    pub(crate) args: &'a [String],
    pub(crate) env: &'a BTreeMap<String, String>,
    pub(crate) inherited: &'a BTreeMap<String, String>,
}

/// A running stdio server and the session on it.
pub(crate) struct StdioServer {
    pub(crate) service: RunningService<RoleClient, Client>,
    pub(crate) child: Child,
    pub(crate) stderr: SharedTail,
}

/// Starts the server and completes the MCP handshake with it.
pub(crate) async fn start(launch: &Launch<'_>, client: Client) -> Result<StdioServer, Failure> {
    let mut child = spawn(launch)?;
    let stderr = SharedTail::default();
    let collector = child.stderr.take().map(|stream| tokio::spawn(stderr.clone().collect(stream)));
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err(Failure::internal("a stdio server's pipes were not captured"));
    };
    match serve_client(client, (stdout, stdin)).await {
        Ok(service) => Ok(StdioServer { service, child, stderr }),
        Err(error) => {
            let failure = start_failure(launch.server, error);
            let exit = settle(&mut child, collector).await;
            let failure = match exit {
                Some(status) => Failure {
                    message: format!(
                        "server `{}` (`{}`) {} before it finished starting",
                        launch.server,
                        launch.command,
                        describe_exit(status)
                    ),
                    ..failure
                },
                None => failure,
            };
            Err(with_stderr(failure, launch.server, stderr.lines()))
        }
    }
}

impl StdioServer {
    /// Ends the session, then the server: its standard input closes, then its process group
    /// gets `SIGTERM`, then `SIGKILL`. The group holds what a launcher such as `npx` started.
    /// Best effort: the daemon has nowhere to report a server it could not signal.
    pub(crate) async fn stop(mut self) {
        // Closing the session closes the transport, which closes the server's standard input.
        let _ = self.service.close_with_timeout(EXIT_GRACE).await;
        if wait_for_exit(&mut self.child, EXIT_GRACE).await {
            return;
        }
        // The server has not been reaped, so its process group still exists.
        let group = self.child.id().and_then(|id| i32::try_from(id).ok()).and_then(Pid::from_raw);
        if let Some(group) = group {
            let _ = rustix::process::kill_process_group(group, Signal::TERM);
            if wait_for_exit(&mut self.child, EXIT_GRACE).await {
                return;
            }
            let _ = rustix::process::kill_process_group(group, Signal::KILL);
        }
        let _ = self.child.kill().await;
    }
}

fn spawn(launch: &Launch<'_>) -> Result<Child, Failure> {
    let workspace = Path::new(launch.workspace);
    if !workspace.is_dir() {
        return Err(Failure::unavailable(format!(
            "cannot start server `{}`: its workspace {} is not a directory",
            launch.server, launch.workspace
        )));
    }
    Command::new(program(launch.command, workspace))
        .args(launch.args)
        .env_clear()
        .envs(launch.inherited)
        .envs(launch.env)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| spawn_failure(launch, &error))
}

/// A relative command with a directory in it names a file in the workspace. `Command` leaves
/// whether such a path resolves against the old or the new working directory unspecified.
fn program(command: &str, workspace: &Path) -> PathBuf {
    let path = Path::new(command);
    if path.is_relative() && command.contains('/') {
        workspace.join(path)
    } else {
        path.to_path_buf()
    }
}

fn spawn_failure(launch: &Launch<'_>, error: &io::Error) -> Failure {
    let message = if error.kind() == io::ErrorKind::NotFound {
        format!(
            "cannot start server `{}`: command `{}` was not found",
            launch.server, launch.command
        )
    } else {
        format!("cannot start server `{}` (`{}`): {error}", launch.server, launch.command)
    };
    let failure = Failure::unavailable(message);
    let failure = if error.kind() == io::ErrorKind::NotFound {
        failure.suggest(format!(
            "install `{}`, or put it on the PATH of the environment that runs mcp",
            launch.command
        ))
    } else {
        failure
    };
    with_stderr(failure, launch.server, Vec::new())
}

/// After a failed handshake: the server's exit status, if it exits within the grace period,
/// once its stderr is read to the end. A server still running is killed.
async fn settle(child: &mut Child, collector: Option<JoinHandle<()>>) -> Option<ExitStatus> {
    let status = match tokio::time::timeout(FAILURE_GRACE, child.wait()).await {
        Ok(Ok(status)) => Some(status),
        Ok(Err(_)) | Err(_) => {
            let _ = child.kill().await;
            None
        }
    };
    if let Some(collector) = collector {
        let _ = tokio::time::timeout(FAILURE_GRACE, collector).await;
    }
    status
}

async fn wait_for_exit(child: &mut Child, grace: Duration) -> bool {
    matches!(tokio::time::timeout(grace, child.wait()).await, Ok(Ok(_)))
}

fn describe_exit(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited with status {code}"),
        (None, Some(signal)) => format!("was killed by signal {signal}"),
        (None, None) => format!("exited ({status})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_resolves_in_the_workspace() {
        let workspace = Path::new("/work/project");
        assert_eq!(program("./bin/server", workspace), Path::new("/work/project/./bin/server"));
        assert_eq!(program("bin/server", workspace), Path::new("/work/project/bin/server"));
        assert_eq!(program("npx", workspace), Path::new("npx"));
        assert_eq!(program("/usr/bin/env", workspace), Path::new("/usr/bin/env"));
    }
}
