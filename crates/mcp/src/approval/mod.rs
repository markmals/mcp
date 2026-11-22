//! Approval of a project's servers by a person. `docs/approval.md` is the contract.

mod ask;
mod store;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

pub use self::ask::ask_person;
pub use self::store::{Approval, ApprovalStore, Approvals, Binding};
use crate::NAME;
use crate::config::{Server, Source, Workspace};
use crate::failure::Failure;

/// The environment variable that turns approval on.
pub const REQUIRE_VARIABLE: &str = "MCP_REQUIRE_APPROVAL";

/// Whether a server's configuration, as written, has a person's approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalState {
    /// A person approved this configuration as it is written.
    Approved,
    /// No approval exists for this server in this repository.
    Unapproved,
    /// A person approved an earlier configuration of this server.
    Changed,
    /// A user-file server, which never needs approval.
    NotRequired,
}

/// Whether `MCP_REQUIRE_APPROVAL` requires approval: `1` yes, unset or `0` no.
///
/// # Errors
///
/// `PRECONDITION_FAILED` for any other value.
pub fn required(env: &impl Fn(&str) -> Option<String>) -> Result<bool, Failure> {
    match env(REQUIRE_VARIABLE).as_deref() {
        None | Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(other) => Err(Failure::precondition(format!(
            "{REQUIRE_VARIABLE} must be 1 (approval required) or 0 (not required), not `{other}`"
        ))
        .with_details(json!({ "variable": REQUIRE_VARIABLE, "value": other }))),
    }
}

/// What an approval of `server` is bound to; `None` for a user server.
pub fn binding(workspace: &Workspace, server: &Server) -> Option<Binding> {
    if server.source != Source::Project {
        return None;
    }
    let (repository, file) = workspace.approval_scope()?;
    Some(Binding { repository, file, server: server.name.clone() })
}

pub fn state(approvals: &Approvals, workspace: &Workspace, server: &Server) -> ApprovalState {
    let Some(binding) = binding(workspace, server) else {
        return ApprovalState::NotRequired;
    };
    let mut fingerprints = approvals.fingerprints(&binding).peekable();
    if fingerprints.peek().is_none() {
        return ApprovalState::Unapproved;
    }
    if fingerprints.any(|fingerprint| fingerprint == server.fingerprint) {
        ApprovalState::Approved
    } else {
        ApprovalState::Changed
    }
}

/// The failure for a server that approval blocks.
pub fn blocked(server: &Server, state: ApprovalState) -> Failure {
    let why = match state {
        ApprovalState::Changed => "its configuration changed since a person approved it",
        _ => "no person has approved it",
    };
    Failure::precondition(format!(
        "project server `{}` needs a person's approval: {why}",
        server.name
    ))
    .with_details(json!({
        "server": server.name,
        "file": server.file.display().to_string(),
        "approval": state,
    }))
    .suggest(format!(
        "A person runs `{NAME} servers approve {}` and confirms it; an agent cannot approve it",
        crate::registry::shell_word(&server.name)
    ))
    .suggest(format!("{NAME} docs show docs/approval.md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_and_zero_are_valid() {
        let with = |value: &'static str| {
            move |key: &str| (key == REQUIRE_VARIABLE).then(|| value.to_string())
        };
        assert!(!required(&|_: &str| None).unwrap());
        assert!(!required(&with("0")).unwrap());
        assert!(required(&with("1")).unwrap());
        for value in ["true", "yes", "", "2"] {
            assert!(required(&with(value)).is_err(), "{value}");
        }
    }
}
