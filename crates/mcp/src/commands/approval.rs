//! `mcp servers approve` and `mcp servers revoke`: a person's approval of a project's servers.

use std::time::Duration;

use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use serde_json::json;

use crate::NAME;
use crate::approval::{self, Approval, ApprovalState, ask_person};
use crate::config::{Server, Source};
use crate::connect::Connector;
use crate::daemon::client;
use crate::daemon::protocol::Request;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{
    ALL_SERVERS, ArgumentKind, ArgumentSpec, CommandSpec, DRY_RUN, Effect, Example, Requirement,
    TIMEOUT, ValueKind,
};
use crate::response::{RenderText, Response};

pub const SERVER_NAMES: ArgumentSpec = ArgumentSpec {
    name: "names",
    kind: ArgumentKind::Positional,
    value: ValueKind::String,
    required: false,
    default: None,
    multiple: true,
    summary: "The servers' names; or pass --all",
};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "servers.approve",
        path: &["servers", "approve"],
        summary: "Ask the person at this machine to approve a project's servers (Touch ID, password)",
        description: "Asks the person with the system's own authentication (Touch ID, Apple Watch, \
            or the login password on macOS; polkit on Linux) to approve the named project \
            servers, or every one with --all, in one request, then records the approvals in \
            mcp's state directory, bound to this repository and to each server's configuration \
            as written. Always asks, even while approval is not required (MCP_REQUIRE_APPROVAL), \
            so servers can be approved first. An agent cannot approve: a missing or declined \
            answer fails with PERMISSION_DENIED and records nothing. User-file servers return \
            `not-required` and servers already approved `already-approved`, without asking. \
            --dry-run lists what the person would be asked to approve and never asks.",
        arguments: &[SERVER_NAMES, ALL_SERVERS, DRY_RUN, TIMEOUT],
        effect: Effect::Modifies("approvals"),
        requires: &[Requirement::Configuration, Requirement::Person],
        dry_run: true,
        idempotent: true,
        errors: &[
            ErrorCode::InvalidArgument,
            ErrorCode::NotFound,
            ErrorCode::PreconditionFailed,
            ErrorCode::PermissionDenied,
            ErrorCode::Timeout,
            ErrorCode::Unavailable,
        ],
        examples: &[Example {
            summary: "Show what approving the server `demo` would ask the person",
            args: &["servers", "approve", "demo", "--dry-run"],
        }],
        synonyms: &["trust", "allow", "authorize", "consent", "biometric", "touch"],
        output_schema: || schema_for!(ApprovalReceipt),
        run: approve,
    },
    CommandSpec {
        id: "servers.revoke",
        path: &["servers", "revoke"],
        summary: "Remove the approvals of a project's servers",
        description: "Removes every approval of the named project servers, or of every server \
            with --all, in this repository. Needs no confirmation. While approval is required \
            (MCP_REQUIRE_APPROVAL=1), also ends the servers' sessions in this workspace. \
            --dry-run reports what would be revoked without changing anything.",
        arguments: &[SERVER_NAMES, ALL_SERVERS, DRY_RUN],
        effect: Effect::Modifies("approvals"),
        requires: &[Requirement::Configuration],
        dry_run: true,
        idempotent: true,
        errors: &[
            ErrorCode::InvalidArgument,
            ErrorCode::NotFound,
            ErrorCode::PreconditionFailed,
            ErrorCode::Unavailable,
        ],
        examples: &[
            Example {
                summary: "Preview revoking every approval",
                args: &["servers", "revoke", "--all", "--dry-run"],
            },
            Example { summary: "Revoke the server `demo`", args: &["servers", "revoke", "demo"] },
        ],
        synonyms: &["untrust", "disallow", "forget", "remove", "unapprove"],
        output_schema: || schema_for!(RevocationReceipt),
        run: revoke,
    },
];

/// Whether a command changed anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiptStatus {
    /// The change was made.
    Completed,
    /// Nothing changed; the receipt shows what would have happened.
    DryRun,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalReceipt {
    pub status: ReceiptStatus,
    /// The file the approvals are kept in.
    pub approvals_file: String,
    pub items: Vec<ApprovalItem>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalItem {
    pub server: String,
    pub file: String,
    pub outcome: ApprovalOutcome,
    /// The configuration approved: what a later change is compared with.
    pub fingerprint: String,
    /// The command line or URL as written.
    pub target: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalOutcome {
    /// The person approved it (with `dry-run`: the person would be asked).
    Approved,
    /// This configuration was already approved; nothing was asked.
    AlreadyApproved,
    /// A user-file server, which never needs approval.
    NotRequired,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevocationReceipt {
    pub status: ReceiptStatus,
    pub approvals_file: String,
    pub items: Vec<RevocationItem>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevocationItem {
    pub server: String,
    /// `revoked`, or `not-approved` when no approval existed.
    pub outcome: RevocationOutcome,
    /// Whether its session in this workspace ended.
    pub disconnected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RevocationOutcome {
    Revoked,
    NotApproved,
}

impl RenderText for ApprovalReceipt {
    fn render_text(&self) -> String {
        let verb = if self.status == ReceiptStatus::DryRun { "would be" } else { "" };
        self.items
            .iter()
            .map(|item| {
                let outcome = match item.outcome {
                    ApprovalOutcome::Approved => format!("{verb} approved"),
                    ApprovalOutcome::AlreadyApproved => "already approved".into(),
                    ApprovalOutcome::NotRequired => "a user server; needs no approval".into(),
                };
                format!("{}: {}\n", item.server, outcome.trim())
            })
            .collect()
    }
}

impl RenderText for RevocationReceipt {
    fn render_text(&self) -> String {
        let prefix = if self.status == ReceiptStatus::DryRun { "would be " } else { "" };
        self.items
            .iter()
            .map(|item| match item.outcome {
                RevocationOutcome::Revoked => format!("{}: {prefix}revoked\n", item.server),
                RevocationOutcome::NotApproved => format!("{}: not approved\n", item.server),
            })
            .collect()
    }
}

/// The servers a command acts on: those named, or every one with `--all`.
///
/// # Errors
///
/// `INVALID_ARGUMENT` when neither or both are given.
pub fn selected_names(invocation: &Invocation) -> Result<Option<Vec<String>>, Failure> {
    let names = invocation.strings("names");
    match (names.is_empty(), invocation.flag("all")) {
        (false, false) => Ok(Some(names)),
        (true, true) => Ok(None),
        (true, false) => Err(Failure::invalid_argument("name at least one server, or pass --all")
            .with_details(json!({ "missing": ["names"] }))
            .suggest(format!("{NAME} servers list"))),
        (false, true) => Err(Failure::invalid_argument("pass server names or --all, not both")
            .with_details(json!({ "argument": "all" }))),
    }
}

fn selected<'a>(
    connector: &'a Connector,
    invocation: &Invocation,
) -> Result<Vec<&'a Server>, Failure> {
    match selected_names(invocation)? {
        Some(names) => names.iter().map(|name| connector.configuration.server(name)).collect(),
        None => Ok(connector.configuration.servers()?.iter().collect()),
    }
}

fn approve(invocation: &Invocation) -> Result<Response, Failure> {
    let connector = Connector::load(invocation)?;
    let servers = selected(&connector, invocation)?;
    let items: Vec<ApprovalItem> = servers
        .iter()
        .map(|server| ApprovalItem {
            server: server.name.clone(),
            file: server.file.display().to_string(),
            outcome: match connector.approval(server) {
                ApprovalState::NotRequired => ApprovalOutcome::NotRequired,
                ApprovalState::Approved => ApprovalOutcome::AlreadyApproved,
                ApprovalState::Unapproved | ApprovalState::Changed => ApprovalOutcome::Approved,
            },
            fingerprint: server.fingerprint.clone(),
            target: server.target(),
        })
        .collect();
    let receipt = |status| ApprovalReceipt {
        status,
        approvals_file: connector.approval_store.path.display().to_string(),
        items,
    };
    let asked: Vec<&Server> = servers
        .iter()
        .copied()
        .filter(|server| {
            matches!(connector.approval(server), ApprovalState::Unapproved | ApprovalState::Changed)
        })
        .collect();
    if invocation.dry_run() || asked.is_empty() {
        let status =
            if invocation.dry_run() { ReceiptStatus::DryRun } else { ReceiptStatus::Completed };
        return Ok(Response::new(&receipt(status)));
    }
    ask_person(
        &reason(&connector, &asked),
        Duration::from_secs(invocation.integer("timeout").unwrap_or(120)),
    )?;
    let approved_at = jiff::Timestamp::now().to_string();
    connector.approval_store.update(|approvals| {
        for server in &asked {
            if let Some(binding) = approval::binding(&connector.configuration.workspace, server) {
                approvals.approvals.push(Approval {
                    binding,
                    fingerprint: server.fingerprint.clone(),
                    target: server.target(),
                    approved_at: approved_at.clone(),
                });
            }
        }
    })?;
    Ok(Response::new(&receipt(ReceiptStatus::Completed)))
}

/// What the system's dialog says the person is approving.
fn reason(connector: &Connector, servers: &[&Server]) -> String {
    let listed: Vec<String> = servers
        .iter()
        .map(|server| format!("{} ({})", server.name, server.target().unwrap_or_default()))
        .collect();
    format!(
        "approve running the MCP servers {} from the project in {}",
        listed.join(", "),
        connector.configuration.workspace.root.display()
    )
}

fn revoke(invocation: &Invocation) -> Result<Response, Failure> {
    let connector = Connector::load(invocation)?;
    let servers: Vec<&Server> = selected(&connector, invocation)?;
    let bindings: Vec<_> = servers
        .iter()
        .map(|server| (server, approval::binding(&connector.configuration.workspace, server)))
        .collect();
    let mut items: Vec<RevocationItem> = bindings
        .iter()
        .map(|(server, binding)| RevocationItem {
            server: server.name.clone(),
            outcome: match binding {
                Some(binding) if connector.approvals.fingerprints(binding).next().is_some() => {
                    RevocationOutcome::Revoked
                }
                _ => RevocationOutcome::NotApproved,
            },
            disconnected: false,
        })
        .collect();
    let status =
        if invocation.dry_run() { ReceiptStatus::DryRun } else { ReceiptStatus::Completed };
    if status == ReceiptStatus::Completed {
        connector.approval_store.update(|approvals| {
            approvals.approvals.retain(|approval| {
                !bindings.iter().any(|(_, binding)| binding.as_ref() == Some(&approval.binding))
            });
        })?;
        if connector.approval_required {
            end_sessions(&connector, &servers, &mut items)?;
        }
    }
    Ok(Response::new(&RevocationReceipt {
        status,
        approvals_file: connector.approval_store.path.display().to_string(),
        items,
    }))
}

/// Ends the revoked project servers' sessions, so a revoked server stops answering at once.
fn end_sessions(
    connector: &Connector,
    servers: &[&Server],
    items: &mut [RevocationItem],
) -> Result<(), Failure> {
    let project: Vec<String> = servers
        .iter()
        .filter(|server| server.source == Source::Project)
        .map(|server| server.name.clone())
        .collect();
    let request = Request::Disconnect { workspace: connector.workspace(), servers: Some(project) };
    if let Some(crate::daemon::protocol::ReplyBody::Disconnected { servers: ended }) =
        client::send_if_running(&connector.daemon, &request, Duration::from_secs(30))?
    {
        for item in items.iter_mut() {
            item.disconnected = ended.contains(&item.server);
        }
    }
    Ok(())
}
