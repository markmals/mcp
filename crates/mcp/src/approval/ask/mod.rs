//! Asking the person at the machine to confirm an approval, with the platform's own
//! authentication: LocalAuthentication on macOS, polkit on Linux.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

use std::time::Duration;

use serde_json::json;

use crate::failure::{ErrorCode, Failure};

/// Asks the person to authenticate to approve what `reason` describes, waiting at most
/// `timeout`.
///
/// # Errors
///
/// `PERMISSION_DENIED` when the person cancels, authentication fails, or the person cannot be
/// asked; `TIMEOUT` when no answer comes within `timeout`, after withdrawing the request.
pub fn ask_person(reason: &str, timeout: Duration) -> Result<(), Failure> {
    #[cfg(target_os = "macos")]
    let outcome = macos::ask(reason, timeout);
    #[cfg(target_os = "linux")]
    let outcome = linux::ask(reason, timeout);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let outcome = {
        let _ = (reason, timeout);
        Err(Refusal::CannotAsk("this system has no way for mcp to ask".into()))
    };
    outcome.map_err(|refusal| refusal.into_failure(timeout))
}

/// Why the person did not approve.
#[derive(Debug)]
enum Refusal {
    Cancelled,
    AuthenticationFailed,
    CannotAsk(String),
    TimedOut,
}

impl Refusal {
    fn into_failure(self, timeout: Duration) -> Failure {
        let (code, reason, message) = match self {
            Refusal::Cancelled => (
                ErrorCode::PermissionDenied,
                "cancelled",
                "the person cancelled the approval".into(),
            ),
            Refusal::AuthenticationFailed => (
                ErrorCode::PermissionDenied,
                "authentication-failed",
                "the person could not be authenticated".into(),
            ),
            Refusal::CannotAsk(why) => (
                ErrorCode::PermissionDenied,
                "cannot-ask",
                format!("mcp cannot ask for approval: {why}"),
            ),
            Refusal::TimedOut => (
                ErrorCode::Timeout,
                "timed-out",
                format!(
                    "no one answered the approval request within {} seconds",
                    timeout.as_secs()
                ),
            ),
        };
        Failure::new(code, format!("{message}; nothing was approved"))
            .with_details(json!({ "reason": reason }))
            .suggest("A person at this machine runs the same command and confirms it")
    }
}
