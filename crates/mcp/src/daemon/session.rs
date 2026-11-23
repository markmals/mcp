//! One session: a connection to one server for one workspace, and the requests made on it.

use std::ops::Deref;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rmcp::model::CallToolRequestParams;
use rmcp::service::{Peer, RoleClient, RunningService};
use serde_json::{Map, Value};

use super::failures::{request_failure, with_stderr};
use super::handler::{Client, ToolCache};
use super::http;
use super::protocol::{ServerDeclaration, SessionInfo, SessionSummary, SessionTarget, Transport};
use super::stderr_tail::SharedTail;
use super::stdio::{self, Launch, StdioServer};
use crate::failure::{ErrorCode, Failure};

/// How long [`Session::stop`] waits for an HTTP session to end before abandoning it.
const HTTP_CLOSE_GRACE: Duration = Duration::from_secs(2);

pub(crate) struct Session {
    id: String,
    workspace: String,
    server: String,
    started_at: String,
    /// The settings the session started with; a request with other settings replaces it.
    pub(crate) transport: Transport,
    peer: Peer<RoleClient>,
    tools: Arc<ToolCache>,
    /// `None` for HTTP servers.
    stderr: Option<SharedTail>,
    activity: Mutex<Activity>,
    /// Taken by [`Session::stop`].
    process: Mutex<Option<Process>>,
}

enum Process {
    Stdio(StdioServer),
    Http(RunningService<RoleClient, Client>),
}

struct Activity {
    /// Requests using the session now.
    active: usize,
    last_used: Instant,
}

impl Session {
    pub(crate) async fn start(target: &SessionTarget, id: String) -> Result<Session, Failure> {
        let client = Client::default();
        let tools = client.tools.clone();
        let (process, stderr) = match &target.transport {
            Transport::Stdio { command, args, env, inherited } => {
                let launch = Launch {
                    server: &target.server,
                    workspace: &target.workspace,
                    command,
                    args,
                    env,
                    inherited,
                };
                let server = stdio::start(&launch, client).await?;
                let stderr = server.stderr.clone();
                (Process::Stdio(server), Some(stderr))
            }
            Transport::Http { url, headers } => {
                (Process::Http(http::start(&target.server, url, headers, client).await?), None)
            }
        };
        let peer = match &process {
            Process::Stdio(server) => server.service.peer().clone(),
            Process::Http(service) => service.peer().clone(),
        };
        Ok(Session {
            id,
            workspace: target.workspace.clone(),
            server: target.server.clone(),
            started_at: jiff::Timestamp::now().to_string(),
            transport: target.transport.clone(),
            peer,
            tools,
            stderr,
            activity: Mutex::new(Activity { active: 0, last_used: Instant::now() }),
            process: Mutex::new(Some(process)),
        })
    }

    /// False once the server closed the connection, as a crashed server does.
    pub(crate) fn is_alive(&self) -> bool {
        !self.peer.is_transport_closed()
    }

    /// How long the session has gone without a request; zero while one is running.
    pub(crate) fn idle_for(&self, now: Instant) -> Duration {
        let activity = lock(&self.activity);
        if activity.active > 0 {
            Duration::ZERO
        } else {
            now.saturating_duration_since(activity.last_used)
        }
    }

    pub(crate) fn begin_use(self: &Arc<Session>) -> SessionUse {
        let mut activity = lock(&self.activity);
        activity.active += 1;
        activity.last_used = Instant::now();
        SessionUse(Arc::clone(self))
    }

    pub(crate) fn info(&self, reused: bool) -> SessionInfo {
        SessionInfo {
            id: self.id.clone(),
            workspace: self.workspace.clone(),
            started_at: self.started_at.clone(),
            reused,
        }
    }

    pub(crate) fn summary(&self) -> SessionSummary {
        SessionSummary {
            workspace: self.workspace.clone(),
            server: self.server.clone(),
            id: self.id.clone(),
            started_at: self.started_at.clone(),
        }
    }

    pub(crate) fn declaration(&self) -> Result<ServerDeclaration, Failure> {
        let info = self.peer.peer_info().ok_or_else(|| {
            Failure::internal(format!("session with server `{}` has no handshake", self.server))
        })?;
        Ok(ServerDeclaration {
            info: info.server_info.as_ref().map(to_value).transpose()?,
            protocol_version: info.protocol_version.to_string(),
            capabilities: to_value(&info.capabilities)?,
            instructions: info.instructions.clone(),
            stderr: self.stderr_lines(),
        })
    }

    /// Every page of the server's tools, from the cache when the server has not announced a
    /// change since they were fetched.
    pub(crate) async fn list_tools(&self) -> Result<Vec<Value>, Failure> {
        let declares_tools =
            self.peer.peer_info().is_some_and(|info| info.capabilities.tools.is_some());
        if !declares_tools {
            return Err(Failure::precondition(format!(
                "server `{}` declares no tools",
                self.server
            ))
            .with_details(serde_json::json!({ "server": self.server })));
        }
        if let Some(tools) = self.tools.tools() {
            return Ok(tools);
        }
        let generation = self.tools.generation();
        let tools = self.peer.list_all_tools().await.map_err(|error| self.failure(error))?;
        let tools = tools.iter().map(to_value).collect::<Result<Vec<_>, _>>()?;
        self.tools.store(generation, &tools);
        Ok(tools)
    }

    /// The server's `CallToolResult`, whether or not it reports an error.
    pub(crate) async fn call_tool(
        &self,
        tool: String,
        arguments: Map<String, Value>,
    ) -> Result<Value, Failure> {
        let params = CallToolRequestParams::new(tool).with_arguments(arguments);
        let result = self.peer.call_tool(params).await.map_err(|error| self.failure(error))?;
        to_value(&result)
    }

    /// Ends the session; a stdio server is then stopped as [`StdioServer::stop`] describes.
    pub(crate) async fn stop(&self) {
        let process = lock(&self.process).take();
        match process {
            Some(Process::Stdio(server)) => server.stop().await,
            Some(Process::Http(mut service)) => {
                let _ = service.close_with_timeout(HTTP_CLOSE_GRACE).await;
            }
            None => {}
        }
    }

    fn stderr_lines(&self) -> Vec<String> {
        self.stderr.as_ref().map(SharedTail::lines).unwrap_or_default()
    }

    fn failure(&self, error: rmcp::ServiceError) -> Failure {
        let failure = request_failure(&self.server, error);
        if failure.code == ErrorCode::Unavailable && self.stderr.is_some() {
            with_stderr(failure, &self.server, self.stderr_lines())
        } else {
            failure
        }
    }
}

/// A request's use of a session; the session counts as idle again when it drops.
pub(crate) struct SessionUse(Arc<Session>);

impl Deref for SessionUse {
    type Target = Session;

    fn deref(&self) -> &Session {
        &self.0
    }
}

impl Drop for SessionUse {
    fn drop(&mut self) {
        let mut activity = lock(&self.0.activity);
        activity.active -= 1;
        activity.last_used = Instant::now();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn to_value(value: &impl serde::Serialize) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|error| {
        Failure::internal(format!("cannot represent a server's answer as JSON: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn stdio(env: &[(&str, &str)], inherited: &[(&str, &str)]) -> Transport {
        let map = |entries: &[(&str, &str)]| -> BTreeMap<String, String> {
            entries.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
        };
        Transport::Stdio {
            command: "server".to_owned(),
            args: vec!["--flag".to_owned()],
            env: map(env),
            inherited: map(inherited),
        }
    }

    #[test]
    fn the_inherited_environment_does_not_change_a_session() {
        let first = stdio(&[("TOKEN", "a")], &[("PATH", "/bin"), ("TERM", "xterm")]);
        let second = stdio(&[("TOKEN", "a")], &[("PATH", "/usr/bin")]);
        assert!(first.same_settings(&second));
    }

    #[test]
    fn configured_settings_do_change_a_session() {
        let first = stdio(&[("TOKEN", "a")], &[]);
        assert!(!first.same_settings(&stdio(&[("TOKEN", "b")], &[])));
        let http =
            Transport::Http { url: "http://localhost/mcp".to_owned(), headers: BTreeMap::new() };
        assert!(!first.same_settings(&http));
    }
}
