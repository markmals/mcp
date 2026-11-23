//! What `mcp` and its session daemon say to each other over the daemon's Unix socket.
//!
//! One connection carries one exchange: the client writes one [`Request`] as a line of JSON, then
//! keeps the connection open until it reads one [`Reply`] line. Closing the connection early,
//! as an interrupted `mcp` does, cancels the request: the daemon cancels any request it sent a
//! server on the client's behalf. Every type here is internal to one `mcp` version; the socket
//! name carries the version, so a client never talks to a daemon of another version.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::failure::ErrorCode;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Request {
    /// The daemon's process id and every session it holds. Never starts a session.
    Status,
    /// Start or reuse the target's session and report what the server declared.
    Connect { target: SessionTarget },
    /// The target server's complete tool list, every page, from the session's cache when the
    /// server has not announced a change since it was fetched.
    ListTools { target: SessionTarget },
    /// Call one tool with `arguments`, exactly as given.
    CallTool { target: SessionTarget, tool: String, arguments: Map<String, Value> },
    /// End sessions in `workspace`: those named in `servers`, or every one when it is `None`.
    Disconnect { workspace: String, servers: Option<Vec<String>> },
}

/// A server, resolved by the client: which session it belongs to and how to start it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTarget {
    /// The canonical workspace directory. Sessions are keyed by workspace and server name.
    pub workspace: String,
    pub server: String,
    pub transport: Transport,
    /// Bounds starting the session and the request together.
    pub timeout_ms: u64,
}

/// A server's connection settings with every `${VAR}` already expanded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Transport {
    Stdio {
        command: String,
        args: Vec<String>,
        /// The server's configured `env`, applied over `inherited`.
        env: BTreeMap<String, String>,
        /// The environment of the `mcp` invocation; a new process starts with exactly this
        /// plus `env`. Not part of the session's identity.
        inherited: BTreeMap<String, String>,
    },
    Http {
        url: String,
        headers: BTreeMap<String, String>,
    },
}

impl Transport {
    /// Whether a session started with `self` can serve a request for `other`: the same
    /// connection settings, ignoring the inherited environment.
    pub fn same_settings(&self, other: &Transport) -> bool {
        match (self, other) {
            (
                Transport::Stdio { command, args, env, .. },
                Transport::Stdio {
                    command: other_command, args: other_args, env: other_env, ..
                },
            ) => command == other_command && args == other_args && env == other_env,
            (Transport::Http { url, headers }, Transport::Http { url: other_url, headers: h }) => {
                url == other_url && headers == h
            }
            _ => false,
        }
    }
}

/// The one line the daemon writes back.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reply {
    Ok(ReplyBody),
    Error(DaemonError),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ReplyBody {
    Status(DaemonStatus),
    Connected {
        session: SessionInfo,
        server: ServerDeclaration,
    },
    Tools {
        session: SessionInfo,
        tools: Vec<Value>,
    },
    /// The server's `CallToolResult` unchanged, `isError` included.
    Called {
        session: SessionInfo,
        result: Value,
    },
    Disconnected {
        servers: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonStatus {
    pub pid: u32,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub workspace: String,
    pub server: String,
    pub id: String,
    pub started_at: String,
}

/// The session a result used.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    /// The workspace the session belongs to.
    pub workspace: String,
    /// When the session started, RFC 3339.
    pub started_at: String,
    /// Whether an earlier command started this session. False means any state from earlier
    /// calls, such as a selected browser page, is gone.
    pub reused: bool,
}

/// What the server declared when the session started.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerDeclaration {
    /// The server's `serverInfo` (`name`, `version`, and the rest), when it sent one.
    pub info: Option<Value>,
    pub protocol_version: String,
    pub capabilities: Value,
    pub instructions: Option<String>,
    /// The last lines the server wrote to stderr, oldest first; empty for HTTP servers.
    pub stderr: Vec<String>,
}

/// A failure, as the client reports it. Messages and details may contain configured secret
/// values; the client redacts them before anything reaches its output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Value,
    pub suggestions: Vec<String>,
}
