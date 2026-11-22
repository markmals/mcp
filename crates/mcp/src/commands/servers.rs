//! `mcp servers list` and `mcp servers info`: the configured servers and what each declares.

use std::collections::BTreeSet;

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::approval::ApprovalState;
use crate::config::{Server, Source, TransportKind};
use crate::connect::Connector;
use crate::daemon::client::{self, DaemonState};
use crate::daemon::protocol::{ServerDeclaration, SessionInfo};
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{
    ArgumentKind, ArgumentSpec, CommandSpec, Effect, Example, Requirement, TIMEOUT, ValueKind,
};
use crate::response::{RenderText, Response};
use crate::search::{self, Page};

pub const SERVER_NAME: ArgumentSpec = ArgumentSpec {
    name: "name",
    kind: ArgumentKind::Positional,
    value: ValueKind::String,
    required: true,
    default: None,
    multiple: false,
    summary: "The server's name, as `mcp servers list` shows it",
};

const LIST_LIMIT: ArgumentSpec = ArgumentSpec {
    name: "limit",
    kind: ArgumentKind::Option,
    value: ValueKind::Integer { min: 1, max: Some(200) },
    required: false,
    default: Some("50"),
    multiple: false,
    summary: "The most items to return",
};

pub const CURSOR: ArgumentSpec = ArgumentSpec {
    name: "cursor",
    kind: ArgumentKind::Option,
    value: ValueKind::String,
    required: false,
    default: None,
    multiple: false,
    summary: "Resume after a previous page: the nextCursor it returned",
};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "servers.list",
        path: &["servers", "list"],
        summary: "List the configured MCP servers, where each comes from, and whether it is usable",
        description: "Reads the user's `$XDG_CONFIG_HOME/.mcp.json` and the project's nearest \
            `.mcp.json`; a project server replaces the user server of the same name. Each item \
            gives the server's name, its `source` (`project` or `user`) and `file`, its \
            `transport`, its `target` (the command line or URL as written), its `approval` state, \
            whether it is `usable` now and the `problem` when it is not, and whether a session is \
            `connected`. Never starts a server or the session daemon. Fails while either \
            configuration file is invalid.",
        arguments: &[LIST_LIMIT, CURSOR],
        effect: Effect::Reads,
        requires: &[Requirement::Configuration],
        dry_run: false,
        idempotent: true,
        errors: &[ErrorCode::InvalidArgument, ErrorCode::PreconditionFailed],
        examples: &[Example {
            summary: "List every configured server",
            args: &["servers", "list"],
        }],
        synonyms: &["show", "configured", "mcpservers", "config"],
        output_schema: || schema_for!(Page<ServerSummary>),
        run: list,
    },
    CommandSpec {
        id: "servers.info",
        path: &["servers", "info"],
        summary: "Show a server's configuration and what it declares: name, version, capabilities",
        description: "Reports everything `servers list` does for one server, plus the names of its \
            `env` and `headers` (never their values), its approval `fingerprint`, and the user \
            file whose server it replaces. When the server is usable, starts or reuses its \
            session and adds `live`: the session and what the server declared (its serverInfo, \
            protocol version, capabilities, instructions, and recent stderr). When it is not, or \
            the connection fails, `live` is null, `problem` says why, and the command still \
            succeeds.",
        arguments: &[SERVER_NAME, TIMEOUT],
        effect: Effect::Connects,
        requires: &[Requirement::Configuration, Requirement::Servers],
        dry_run: false,
        idempotent: true,
        errors: &[ErrorCode::InvalidArgument, ErrorCode::NotFound, ErrorCode::PreconditionFailed],
        examples: &[Example {
            summary: "Inspect the server `demo`",
            args: &["servers", "info", "demo"],
        }],
        synonyms: &["get", "show", "inspect", "describe", "capabilities", "version", "status"],
        output_schema: || schema_for!(ServerDetail),
        run: info,
    },
];

/// One configured server.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerSummary {
    pub name: String,
    pub source: Source,
    /// The configuration file that declares it.
    pub file: String,
    /// Null when the entry is too malformed to say.
    pub transport: Option<TransportKind>,
    /// The command line or URL as written, before `${VAR}` expansion.
    pub target: Option<String>,
    pub approval: ApprovalState,
    /// Whether mcp can connect to it now.
    pub usable: bool,
    /// Why it is not usable; null when it is.
    pub problem: Option<String>,
    /// Whether the session daemon holds a session for it in this workspace.
    pub connected: bool,
}

/// One server in full.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerDetail {
    #[serde(flatten)]
    pub summary: ServerSummary,
    /// The names of its `env` variables; values are never shown.
    pub env: Vec<String>,
    /// The names of its `headers`; values are never shown.
    pub headers: Vec<String>,
    /// What an approval of this configuration is bound to.
    pub fingerprint: String,
    /// The user file whose server of the same name this project server replaces.
    pub replaces: Option<String>,
    /// The session and what the server declared; null when mcp did not connect.
    pub live: Option<Live>,
}

#[derive(Serialize, JsonSchema)]
pub struct Live {
    pub session: SessionInfo,
    pub server: ServerDeclaration,
}

impl RenderText for Page<ServerSummary> {
    fn render_text(&self) -> String {
        if self.items.is_empty() {
            return "No servers are configured. `mcp context show` lists the files mcp reads.\n"
                .to_string();
        }
        let width = self.items.iter().map(|item| item.name.len()).max().unwrap_or(0);
        let mut text = String::new();
        for item in &self.items {
            let state = item.problem.as_deref().unwrap_or(if item.connected {
                "connected"
            } else {
                "ready"
            });
            text.push_str(&format!(
                "{:width$}  {:7}  {}  ({state})\n",
                item.name,
                source_label(item.source),
                item.target.as_deref().unwrap_or("?"),
            ));
        }
        if self.has_more {
            text.push_str("More servers follow; pass --cursor with nextCursor.\n");
        }
        text
    }
}

impl RenderText for ServerDetail {
    fn render_text(&self) -> String {
        let summary = &self.summary;
        let mut text = format!(
            "{} ({} server from {})\n  {}\n  approval: {}\n",
            summary.name,
            source_label(summary.source),
            summary.file,
            summary.target.as_deref().unwrap_or("?"),
            serde_json::to_value(summary.approval).unwrap_or_default().as_str().unwrap_or_default(),
        );
        if let Some(problem) = &summary.problem {
            text.push_str(&format!("  problem: {problem}\n"));
        }
        if let Some(live) = &self.live {
            let info = live.server.info.as_ref();
            let name = info.and_then(|info| info["name"].as_str()).unwrap_or("?");
            let version = info.and_then(|info| info["version"].as_str()).unwrap_or("?");
            text.push_str(&format!(
                "  server: {name} {version}, protocol {}\n  session: {}{}\n",
                live.server.protocol_version,
                live.session.id,
                if live.session.reused { " (reused)" } else { " (new)" }
            ));
            if let Some(instructions) = &live.server.instructions {
                text.push_str(&format!("  instructions: {instructions}\n"));
            }
        }
        text
    }
}

fn source_label(source: Source) -> &'static str {
    match source {
        Source::Project => "project",
        Source::User => "user",
    }
}

fn list(invocation: &Invocation) -> Result<Response, Failure> {
    let connector = Connector::load(invocation)?;
    let connected = connected_servers(&connector)?;
    let summaries: Vec<ServerSummary> = connector
        .configuration
        .servers()?
        .iter()
        .map(|server| summarize(&connector, server, &connected))
        .collect();
    let limit = invocation.integer("limit").unwrap_or(50);
    Ok(Response::new(&search::page(summaries, invocation.string("cursor").as_deref(), limit)?))
}

fn info(invocation: &Invocation) -> Result<Response, Failure> {
    let connector = Connector::load(invocation)?;
    let name = invocation.string("name").unwrap_or_default();
    let server = connector.configuration.server(&name)?;
    let connected = connected_servers(&connector)?;
    let mut summary = summarize(&connector, server, &connected);
    let live = if summary.usable {
        match connector.connect(server) {
            Ok((session, declaration)) => {
                summary.connected = true;
                Some(Live { session, server: declaration })
            }
            Err(failure) => {
                summary.problem = Some(failure.message);
                None
            }
        }
    } else {
        None
    };
    Ok(Response::new(&ServerDetail {
        summary,
        env: server.variable_names("env"),
        headers: server.variable_names("headers"),
        fingerprint: server.fingerprint.clone(),
        replaces: server.replaces.as_ref().map(|path| path.display().to_string()),
        live,
    }))
}

fn summarize(
    connector: &Connector,
    server: &Server,
    connected: &BTreeSet<String>,
) -> ServerSummary {
    let problem = connector.prepare(server).err().map(|failure| failure.message);
    ServerSummary {
        name: server.name.clone(),
        source: server.source,
        file: server.file.display().to_string(),
        transport: server.entry.as_ref().ok().map(crate::config::Entry::transport),
        target: server.target(),
        approval: connector.approval(server),
        usable: problem.is_none(),
        problem,
        connected: connected.contains(&server.name),
    }
}

/// The servers with a session in this workspace, from a daemon that is already running.
fn connected_servers(connector: &Connector) -> Result<BTreeSet<String>, Failure> {
    let workspace = connector.workspace();
    Ok(match client::status(&connector.daemon)? {
        DaemonState::Running(status) => status
            .sessions
            .into_iter()
            .filter(|session| session.workspace == workspace)
            .map(|session| session.server)
            .collect(),
        DaemonState::Stopped => BTreeSet::new(),
    })
}
