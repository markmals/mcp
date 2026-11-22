//! LocalAuthentication's `deviceOwnerAuthentication`: Touch ID, Apple Watch, or the login
//! password, in the system's own dialog.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAError, LAPolicy};

use super::Refusal;

pub(super) fn ask(reason: &str, timeout: Duration) -> Result<(), Refusal> {
    // SAFETY: `LAContext::new` has no preconditions.
    let context = unsafe { LAContext::new() };
    let policy = LAPolicy::DeviceOwnerAuthentication;
    // SAFETY: `context` is a live LAContext and `policy` a defined LAPolicy value.
    unsafe { context.canEvaluatePolicy_error(policy) }
        .map_err(|error| Refusal::CannotAsk(error.localizedDescription().to_string()))?;

    let (sender, receiver) = mpsc::channel();
    let keep_alive = context.clone();
    let reply = RcBlock::new(move |success: Bool, error: *mut NSError| {
        let _context = &keep_alive;
        let outcome = if success.as_bool() {
            Ok(())
        } else if error.is_null() {
            Err(Refusal::CannotAsk("the system gave no reason".into()))
        } else {
            // SAFETY: LocalAuthentication passes a valid NSError when it reports failure, and
            // it outlives this block's invocation.
            Err(refusal(unsafe { &*error }))
        };
        let _ = sender.send(outcome);
    });
    // An empty reason makes LocalAuthentication raise an Objective-C exception.
    let reason = if reason.is_empty() { "approve MCP servers" } else { reason };
    // SAFETY: the context, reason, and reply block are valid; the block owns a strong reference
    // to the context, which must stay alive until LocalAuthentication calls it.
    unsafe {
        context.evaluatePolicy_localizedReason_reply(policy, &NSString::from_str(reason), &reply);
    }
    match receiver.recv_timeout(timeout) {
        Ok(outcome) => outcome,
        Err(_) => {
            // SAFETY: withdrawing a pending evaluation is what `invalidate` is for.
            unsafe { context.invalidate() };
            Err(Refusal::TimedOut)
        }
    }
}

fn refusal(error: &NSError) -> Refusal {
    match LAError(error.code()) {
        LAError::UserCancel
        | LAError::SystemCancel
        | LAError::AppCancel
        | LAError::UserFallback => Refusal::Cancelled,
        LAError::AuthenticationFailed => Refusal::AuthenticationFailed,
        _ => Refusal::CannotAsk(error.localizedDescription().to_string()),
    }
}
