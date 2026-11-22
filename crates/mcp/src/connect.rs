//! Reaching configured servers: the configuration and approvals that decide whether a server
//! may be used, and the session daemon that holds its connection.

use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::NAME;
use crate::approval::{self, ApprovalState, ApprovalStore, Approvals};
use crate::config::{Configuration, Server};
use crate::daemon::client;
use crate::daemon::paths::DaemonPaths;
use crate::daemon::protocol::{
    ReplyBody, Request, ServerDeclaration, SessionInfo, SessionTarget, Transport,
};
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::redact;

/// How much longer than `--timeout` the client waits for the daemon, so the daemon's own
/// timeout, which cancels the server's request, answers first.
const REPLY_MARGIN: Duration = Duration::from_secs(5);

pub struct Connector<'a> {
    pub configuration: Configuration,
    pub approvals: Approvals,
    pub approval_store: ApprovalStore,
    pub approval_required: bool,
    pub daemon: DaemonPaths,
    invocation: &'a Invocation<'a>,
}

/// A server's session target, and the values its errors must not show.
pub struct Prepared {
    pub target: SessionTarget,
    pub secrets: Vec<String>,
}

impl<'a> Connector<'a> {
    /// Reads the configuration and approvals for `invocation`.
    ///
    /// # Errors
    ///
    /// `PRECONDITION_FAILED` when the configuration or approvals cannot be located or read,
    /// or `MCP_REQUIRE_APPROVAL` is invalid.
    pub fn load(invocation: &'a Invocation<'a>) -> Result<Connector<'a>, Failure> {
        let env = |key: &str| invocation.env(key);
        let approval_required = approval::required(&env)?;
        let configuration = Configuration::load(&invocation.cwd, &env)?;
        let approval_store = ApprovalStore::locate(&env)?;
        let approvals = approval_store.load()?;
        Ok(Connector {
            configuration,
            approvals,
            approval_store,
            approval_required,
            daemon: DaemonPaths::resolve(env),
            invocation,
        })
    }

    pub fn workspace(&self) -> String {
        self.configuration.workspace.root.display().to_string()
    }

    pub fn approval(&self, server: &Server) -> ApprovalState {
        approval::state(&self.approvals, &self.configuration.workspace, server)
    }

    /// Everything that keeps `server` from being used now: an unusable entry, or missing
    /// approval while approval is required. Unset variables surface in [`Connector::prepare`].
    pub fn blocker(&self, server: &Server) -> Option<Failure> {
        if let Some(problem) = server.problem() {
            return Some(problem);
        }
        let state = self.approval(server);
        let blocked = matches!(state, ApprovalState::Unapproved | ApprovalState::Changed);
        (self.approval_required && blocked).then(|| approval::blocked(server, state))
    }

    /// The session target for `server`.
    ///
    /// # Errors
    ///
    /// `PRECONDITION_FAILED` when the server is unusable, unapproved while approval is
    /// required, or uses an unset variable.
    pub fn prepare(&self, server: &Server) -> Result<Prepared, Failure> {
        if let Some(failure) = self.blocker(server) {
            return Err(failure);
        }
        let env = |key: &str| self.invocation.env(key);
        let resolved = server.resolve(&self.configuration.workspace.root, &env)?;
        let transport = match resolved.transport {
            Transport::Stdio { command, args, env, .. } => {
                Transport::Stdio { command, args, env, inherited: self.invocation.environment() }
            }
            http => http,
        };
        Ok(Prepared {
            target: SessionTarget {
                workspace: self.workspace(),
                server: server.name.clone(),
                transport,
                timeout_ms: u64::try_from(self.timeout().as_millis()).unwrap_or(u64::MAX),
            },
            secrets: resolved.secrets,
        })
    }

    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.invocation.integer("timeout").unwrap_or(120))
    }

    /// Starts or reuses `server`'s session and reports what the server declared.
    ///
    /// # Errors
    ///
    /// Whatever [`Connector::prepare`] or the daemon reports, redacted.
    pub fn connect(&self, server: &Server) -> Result<(SessionInfo, ServerDeclaration), Failure> {
        let prepared = self.prepare(server)?;
        let request = Request::Connect { target: prepared.target.clone() };
        match self.send(&request, &prepared.secrets)? {
            ReplyBody::Connected { session, server: mut declaration } => {
                declaration.stderr = declaration
                    .stderr
                    .iter()
                    .map(|line| redact::text(line, &prepared.secrets))
                    .collect();
                Ok((session, declaration))
            }
            other => Err(unexpected(&other)),
        }
    }

    /// `server`'s complete tool list.
    ///
    /// # Errors
    ///
    /// Whatever [`Connector::prepare`] or the daemon reports, redacted.
    pub fn tools(&self, server: &Server) -> Result<(SessionInfo, Vec<Value>), Failure> {
        let prepared = self.prepare(server)?;
        match self.send(&Request::ListTools { target: prepared.target }, &prepared.secrets)? {
            ReplyBody::Tools { session, tools } => Ok((session, tools)),
            other => Err(unexpected(&other)),
        }
    }

    /// The tool called `name` on `server`, as the server declared it.
    ///
    /// # Errors
    ///
    /// `NOT_FOUND` when the server has no such tool, and whatever [`Connector::tools`] reports.
    pub fn tool(&self, server: &Server, name: &str) -> Result<(SessionInfo, Value), Failure> {
        let (session, tools) = self.tools(server)?;
        let found =
            tools.iter().find(|tool| tool.get("name").and_then(Value::as_str) == Some(name));
        match found {
            Some(tool) => Ok((session, tool.clone())),
            None => {
                let names: Vec<&str> =
                    tools.iter().filter_map(|tool| tool["name"].as_str()).collect();
                Err(Failure::not_found(format!("server `{}` has no tool `{name}`", server.name))
                    .with_details(json!({ "server": server.name, "tool": name, "tools": names }))
                    .suggest(format!(
                        "{NAME} tools list --server {}",
                        crate::registry::shell_word(&server.name)
                    )))
            }
        }
    }

    /// Calls `tool` with `arguments` and returns the server's `CallToolResult` unchanged.
    ///
    /// # Errors
    ///
    /// Whatever [`Connector::prepare`] or the daemon reports, redacted.
    pub fn call(
        &self,
        server: &Server,
        tool: &str,
        arguments: Map<String, Value>,
    ) -> Result<(SessionInfo, Value), Failure> {
        let prepared = self.prepare(server)?;
        let request =
            Request::CallTool { target: prepared.target, tool: tool.to_string(), arguments };
        match self.send(&request, &prepared.secrets)? {
            ReplyBody::Called { session, result } => Ok((session, result)),
            other => Err(unexpected(&other)),
        }
    }

    fn send(&self, request: &Request, secrets: &[String]) -> Result<ReplyBody, Failure> {
        client::send(&self.daemon, request, self.timeout() + REPLY_MARGIN)
            .map_err(|failure| redact::failure(failure, secrets))
    }
}

fn unexpected(reply: &ReplyBody) -> Failure {
    let kind = match reply {
        ReplyBody::Status(_) => "status",
        ReplyBody::Connected { .. } => "connected",
        ReplyBody::Tools { .. } => "tools",
        ReplyBody::Called { .. } => "called",
        ReplyBody::Disconnected { .. } => "disconnected",
    };
    Failure::new(ErrorCode::Internal, format!("the session daemon answered with a `{kind}` reply"))
}
