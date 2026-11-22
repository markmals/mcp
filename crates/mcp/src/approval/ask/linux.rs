//! polkit: the session's authentication agent asks for the user's own password, or a
//! fingerprint where the system offers one, for the action `docs/approval.md` installs.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use zbus::blocking::Connection;
use zbus_polkit::policykit1::{AuthorityProxyBlocking, CheckAuthorizationFlags, Subject};

use super::Refusal;

const ACTION: &str = "io.github.markmals.mcp.approve-servers";

/// polkit shows the action's own message; a caller that is not root cannot replace it, so
/// `reason` goes unused here.
pub(super) fn ask(_reason: &str, timeout: Duration) -> Result<(), Refusal> {
    let connection = Connection::system().map_err(|error| {
        Refusal::CannotAsk(format!("cannot reach polkit on the system bus: {error}"))
    })?;
    let cancellation = format!("mcp-{}", std::process::id());
    let (sender, receiver) = mpsc::channel();
    let checking = connection.clone();
    let id = cancellation.clone();
    std::thread::spawn(move || {
        let _ = sender.send(check(&checking, &id));
    });
    match receiver.recv_timeout(timeout) {
        Ok(outcome) => outcome,
        Err(_) => {
            if let Ok(authority) = AuthorityProxyBlocking::new(&connection) {
                let _ = authority.cancel_check_authorization(&cancellation);
            }
            Err(Refusal::TimedOut)
        }
    }
}

fn check(connection: &Connection, cancellation: &str) -> Result<(), Refusal> {
    let authority = AuthorityProxyBlocking::new(connection)
        .map_err(|error| Refusal::CannotAsk(format!("cannot reach polkit: {error}")))?;
    let subject = Subject::new_for_owner(std::process::id(), None, None).map_err(|error| {
        Refusal::CannotAsk(format!("cannot identify this process to polkit: {error}"))
    })?;
    let result = authority
        .check_authorization(
            &subject,
            ACTION,
            &HashMap::new(),
            CheckAuthorizationFlags::AllowUserInteraction.into(),
            cancellation,
        )
        .map_err(refusal)?;
    if result.is_authorized {
        Ok(())
    } else if result.is_challenge {
        Err(Refusal::CannotAsk("no polkit authentication agent is running in this session".into()))
    } else {
        Err(Refusal::AuthenticationFailed)
    }
}

fn refusal(error: zbus::Error) -> Refusal {
    let name = match &error {
        zbus::Error::MethodError(name, _, _) => name.as_str().to_string(),
        _ => String::new(),
    };
    match name.as_str() {
        "org.freedesktop.PolicyKit1.Error.Cancelled" => Refusal::Cancelled,
        "org.freedesktop.PolicyKit1.Error.NotAuthorized" => Refusal::AuthenticationFailed,
        _ => Refusal::CannotAsk(format!(
            "polkit refused the check ({error}); is the action {ACTION} installed? See \
             `mcp docs show docs/approval.md`"
        )),
    }
}
