//! `mcp servers disconnect`: ending server sessions in this workspace.

use std::time::Duration;

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::commands::approval::{ReceiptStatus, SERVER_NAMES, selected_names};
use crate::config::Configuration;
use crate::daemon::client::{self, DaemonState};
use crate::daemon::paths::DaemonPaths;
use crate::daemon::protocol::{ReplyBody, Request};
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{ALL_SERVERS, CommandSpec, DRY_RUN, Effect, Example, Requirement};
use crate::response::{RenderText, Response};

/// How long ending sessions may take: a stdio server gets a few seconds to exit before it is
/// signalled.
const DISCONNECT_WAIT: Duration = Duration::from_secs(30);

pub static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "servers.disconnect",
    path: &["servers", "disconnect"],
    summary: "End server sessions in this workspace; the next command starts a fresh one",
    description: "Ends the sessions of the named servers, or of every server with --all, in \
        this workspace: a stdio server's process is shut down (its stdin closed, then SIGTERM, \
        then SIGKILL). The next command that needs the server starts a new session, without any \
        state the old one held. Never starts the session daemon; with no daemon running, every \
        server is `not-connected`. Works even while a configuration file is invalid.",
    arguments: &[SERVER_NAMES, ALL_SERVERS, DRY_RUN],
    effect: Effect::Modifies("sessions"),
    requires: &[Requirement::Servers],
    dry_run: true,
    idempotent: true,
    errors: &[ErrorCode::InvalidArgument, ErrorCode::PreconditionFailed, ErrorCode::Unavailable],
    examples: &[
        Example {
            summary: "Preview ending every session in this workspace",
            args: &["servers", "disconnect", "--all", "--dry-run"],
        },
        Example {
            summary: "End the server `demo`'s session",
            args: &["servers", "disconnect", "demo"],
        },
    ],
    synonyms: &["stop", "restart", "kill", "close", "end", "reset", "quit"],
    output_schema: || schema_for!(DisconnectReceipt),
    run: disconnect,
}];

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectReceipt {
    pub status: ReceiptStatus,
    pub workspace: String,
    pub items: Vec<DisconnectItem>,
}

#[derive(Serialize, JsonSchema)]
pub struct DisconnectItem {
    pub server: String,
    pub outcome: DisconnectOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DisconnectOutcome {
    /// Its session ended (with `dry-run`: would end).
    Disconnected,
    /// It had no session in this workspace.
    NotConnected,
}

impl RenderText for DisconnectReceipt {
    fn render_text(&self) -> String {
        if self.items.is_empty() {
            return "No sessions in this workspace.\n".to_string();
        }
        let prefix = if self.status == ReceiptStatus::DryRun { "would be " } else { "" };
        self.items
            .iter()
            .map(|item| match item.outcome {
                DisconnectOutcome::Disconnected => {
                    format!("{}: {prefix}disconnected\n", item.server)
                }
                DisconnectOutcome::NotConnected => format!("{}: not connected\n", item.server),
            })
            .collect()
    }
}

fn disconnect(invocation: &Invocation) -> Result<Response, Failure> {
    let env = |key: &str| invocation.env(key);
    let names = selected_names(invocation)?;
    let configuration = Configuration::load(&invocation.cwd, &env)?;
    let workspace = configuration.workspace.root.display().to_string();
    let paths = DaemonPaths::resolve(env);
    let ended: Vec<String> = if invocation.dry_run() {
        match client::status(&paths)? {
            DaemonState::Running(status) => status
                .sessions
                .into_iter()
                .filter(|session| session.workspace == workspace)
                .map(|session| session.server)
                .filter(|server| names.as_ref().is_none_or(|names| names.contains(server)))
                .collect(),
            DaemonState::Stopped => Vec::new(),
        }
    } else {
        let request = Request::Disconnect { workspace: workspace.clone(), servers: names.clone() };
        match client::send_if_running(&paths, &request, DISCONNECT_WAIT)? {
            Some(ReplyBody::Disconnected { servers }) => servers,
            _ => Vec::new(),
        }
    };
    let mut items: Vec<DisconnectItem> = match &names {
        Some(names) => names
            .iter()
            .map(|server| DisconnectItem {
                server: server.clone(),
                outcome: if ended.contains(server) {
                    DisconnectOutcome::Disconnected
                } else {
                    DisconnectOutcome::NotConnected
                },
            })
            .collect(),
        None => ended
            .iter()
            .map(|server| DisconnectItem {
                server: server.clone(),
                outcome: DisconnectOutcome::Disconnected,
            })
            .collect(),
    };
    items.sort_by(|left, right| left.server.cmp(&right.server));
    let status =
        if invocation.dry_run() { ReceiptStatus::DryRun } else { ReceiptStatus::Completed };
    Ok(Response::new(&DisconnectReceipt { status, workspace, items }))
}
