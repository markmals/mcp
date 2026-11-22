//! `mcp context show`: the configuration, workspace, approvals, and daemon commands use.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::NAME;
use crate::approval::{self, ApprovalStore, REQUIRE_VARIABLE};
use crate::config::{Configuration, Source, WorkspaceReason};
use crate::daemon::client::{self, DaemonState};
use crate::daemon::paths::DaemonPaths;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{CommandSpec, Effect, Example, Requirement};
use crate::response::{RenderText, Response};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "context.show",
    path: &["context", "show"],
    summary: "Show the configuration files, workspace, approval setting, and session daemon in use",
    description: "Reports the workspace and why it was chosen; each configuration file mcp \
        reads (the user's `$XDG_CONFIG_HOME/.mcp.json` and the project's nearest `.mcp.json`) \
        with whether it exists, how many servers it declares, and any problem with it; whether \
        approval is required (MCP_REQUIRE_APPROVAL) and where approvals are kept; and the \
        session daemon's socket, whether it is running, and how many sessions it holds. Works \
        while a configuration file is invalid, and never starts the daemon or a server.",
    arguments: &[],
    effect: Effect::Reads,
    requires: &[Requirement::Configuration],
    dry_run: false,
    idempotent: true,
    errors: &[ErrorCode::PreconditionFailed],
    examples: &[Example { summary: "Show what mcp reads and where", args: &["context", "show"] }],
    synonyms: &[
        "where",
        "config",
        "configuration",
        "file",
        "path",
        "environment",
        "status",
        "daemon",
    ],
    output_schema: || schema_for!(Context),
    run: show_context,
}];

#[derive(Serialize, JsonSchema)]
pub struct Context {
    /// The mcp version answering.
    pub version: String,
    pub workspace: WorkspaceContext,
    /// The user file, then the project file when one was found.
    pub files: Vec<FileContext>,
    pub approval: ApprovalContext,
    pub daemon: DaemonContext,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceContext {
    /// The directory sessions belong to and stdio servers start in.
    pub path: String,
    pub reason: WorkspaceReason,
    /// The root of the enclosing git worktree; null outside git.
    pub git_worktree: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct FileContext {
    pub source: Source,
    pub path: String,
    pub exists: bool,
    /// How many servers it declares.
    pub servers: usize,
    /// Why it is invalid or unreadable; null when it is fine or absent.
    pub problem: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct ApprovalContext {
    /// Whether a project's servers need a person's approval.
    pub required: bool,
    /// The environment variable that sets `required`.
    pub variable: String,
    /// The file approvals are kept in.
    pub file: String,
    /// Why MCP_REQUIRE_APPROVAL or the approvals file is unusable; null when both are fine.
    pub problem: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct DaemonContext {
    pub socket: String,
    pub state: DaemonRunState,
    pub pid: Option<u32>,
    /// Sessions it holds in every workspace.
    pub sessions: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonRunState {
    /// It answers on its socket.
    Running,
    /// Nothing answers; the next command that needs a session starts it.
    Stopped,
}

impl RenderText for Context {
    fn render_text(&self) -> String {
        let mut text = format!("{NAME} {}\nWorkspace: {}\n", self.version, self.workspace.path);
        for file in &self.files {
            let label = match file.source {
                Source::User => "User file",
                Source::Project => "Project file",
            };
            let state = match (&file.problem, file.exists) {
                (Some(problem), _) => format!("invalid: {problem}"),
                (None, true) if file.servers == 1 => "1 server".to_string(),
                (None, true) => format!("{} servers", file.servers),
                (None, false) => "absent".to_string(),
            };
            text.push_str(&format!("{label}: {} ({state})\n", file.path));
        }
        text.push_str(&format!(
            "Approval: {} ({}={})\n",
            if self.approval.required { "required" } else { "not required" },
            self.approval.variable,
            if self.approval.required { "1" } else { "unset or 0" },
        ));
        if let Some(problem) = &self.approval.problem {
            text.push_str(&format!("  problem: {problem}\n"));
        }
        let daemon = match self.daemon.state {
            DaemonRunState::Running => format!("running, {} sessions", self.daemon.sessions),
            DaemonRunState::Stopped => "stopped".to_string(),
        };
        text.push_str(&format!("Daemon: {daemon} ({})\n", self.daemon.socket));
        text
    }
}

fn show_context(invocation: &Invocation) -> Result<Response, Failure> {
    let env = |key: &str| invocation.env(key);
    let configuration = Configuration::load(&invocation.cwd, &env)?;
    let workspace = &configuration.workspace;
    let files = configuration
        .files()
        .map(|file| FileContext {
            source: file.source,
            path: file.display(),
            exists: file.exists,
            servers: file.servers.len(),
            problem: file.problem.clone(),
        })
        .collect();
    let store = ApprovalStore::locate(&env)?;
    let required = approval::required(&env);
    let problem = match (&required, store.load()) {
        (Err(failure), _) => Some(failure.message.clone()),
        (Ok(_), Err(failure)) => Some(failure.message),
        (Ok(_), Ok(_)) => None,
    };
    let paths = DaemonPaths::resolve(env);
    let (state, pid, sessions) = match client::status(&paths)? {
        DaemonState::Running(status) => {
            (DaemonRunState::Running, Some(status.pid), status.sessions.len())
        }
        DaemonState::Stopped => (DaemonRunState::Stopped, None, 0),
    };
    Ok(Response::new(&Context {
        version: env!("CARGO_PKG_VERSION").to_string(),
        workspace: WorkspaceContext {
            path: workspace.root.display().to_string(),
            reason: workspace.reason,
            git_worktree: workspace
                .worktree
                .as_ref()
                .map(|worktree| worktree.root.display().to_string()),
        },
        files,
        approval: ApprovalContext {
            required: required.unwrap_or(false),
            variable: REQUIRE_VARIABLE.to_string(),
            file: store.path.display().to_string(),
            problem,
        },
        daemon: DaemonContext { socket: paths.socket.display().to_string(), state, pid, sessions },
    }))
}
