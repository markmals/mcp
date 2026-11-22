//! The structured error every failing command reports on stderr.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

/// A stable error code. Codes, exit statuses, and retryability are part of the CLI contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Input is missing or malformed.
    InvalidArgument,
    /// A named note, command, or document does not exist.
    NotFound,
    /// The store is not ready for the operation.
    PreconditionFailed,
    /// The operation conflicts with the store's current state.
    Conflict,
    /// The store is busy; retrying can succeed.
    Unavailable,
    /// A defect in `mcp` itself, or a stdout that cannot take the result.
    Internal,
}

impl ErrorCode {
    /// Every code, in the order the reference lists them.
    pub const ALL: [ErrorCode; 6] = [
        ErrorCode::InvalidArgument,
        ErrorCode::NotFound,
        ErrorCode::PreconditionFailed,
        ErrorCode::Conflict,
        ErrorCode::Unavailable,
        ErrorCode::Internal,
    ];

    /// The code as it appears in the error JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidArgument => "INVALID_ARGUMENT",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::PreconditionFailed => "PRECONDITION_FAILED",
            ErrorCode::Conflict => "CONFLICT",
            ErrorCode::Unavailable => "UNAVAILABLE",
            ErrorCode::Internal => "INTERNAL",
        }
    }

    /// The process exit status the code sets. Success is always 0.
    pub fn exit_status(self) -> u8 {
        match self {
            ErrorCode::InvalidArgument => 2,
            ErrorCode::NotFound | ErrorCode::PreconditionFailed => 3,
            ErrorCode::Conflict => 4,
            ErrorCode::Unavailable => 5,
            ErrorCode::Internal => 70,
        }
    }

    /// Whether running the same command again, unchanged, can succeed.
    pub fn retryable(self) -> bool {
        matches!(self, ErrorCode::Unavailable)
    }

    /// One sentence for the reference and `schema`.
    pub fn meaning(self) -> &'static str {
        match self {
            ErrorCode::InvalidArgument => {
                "Input is missing or malformed; `details` names the argument and what it accepts."
            }
            ErrorCode::NotFound => "A named note, command, or document does not exist.",
            ErrorCode::PreconditionFailed => {
                "The store is not ready: it needs `mcp store migrate`, was written by a newer \
                 mcp, or is not a mcp store."
            }
            ErrorCode::Conflict => {
                "The operation conflicts with the store's current state: the name is taken, or the \
                 note's revision is not the one passed to --expect-revision."
            }
            ErrorCode::Unavailable => {
                "Another process holds the store's write lock; retry the same command."
            }
            ErrorCode::Internal => {
                "A defect in mcp itself, or a stdout that cannot take the result; `message` \
                 says which."
            }
        }
    }
}

/// A failed operation: what the agent needs to decide whether and how to recover.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub code: ErrorCode,
    pub message: String,
    /// Boxed so `Result<_, Failure>` stays small on the success path.
    pub details: Box<Value>,
    pub suggestions: Vec<String>,
}

impl Failure {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Failure {
        Failure {
            code,
            message: message.into(),
            details: Box::new(json!({})),
            suggestions: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: Value) -> Failure {
        self.details = Box::new(details);
        self
    }

    #[must_use]
    pub fn suggest(mut self, suggestion: impl Into<String>) -> Failure {
        self.suggestions.push(suggestion.into());
        self
    }

    pub fn invalid_argument(message: impl Into<String>) -> Failure {
        Failure::new(ErrorCode::InvalidArgument, message)
    }

    pub fn not_found(message: impl Into<String>) -> Failure {
        Failure::new(ErrorCode::NotFound, message)
    }

    pub fn precondition(message: impl Into<String>) -> Failure {
        Failure::new(ErrorCode::PreconditionFailed, message)
    }

    pub fn conflict(message: impl Into<String>) -> Failure {
        Failure::new(ErrorCode::Conflict, message)
    }

    pub fn internal(message: impl Into<String>) -> Failure {
        Failure::new(ErrorCode::Internal, message)
    }

    /// The document written to stderr; [`ErrorDocument`] is its schema.
    pub fn to_json(&self) -> Value {
        json!({
            "error": {
                "code": self.code.as_str(),
                "message": self.message,
                "retryable": self.code.retryable(),
                "details": self.details,
                "suggestions": self.suggestions,
            }
        })
    }
}

/// What a failing command writes to stderr, as one JSON document.
#[derive(Serialize, JsonSchema)]
pub struct ErrorDocument {
    pub error: ErrorBody,
}

#[derive(Serialize, JsonSchema)]
pub struct ErrorBody {
    /// Stable code; branch on this, not on the message.
    pub code: ErrorCode,
    /// One sentence for a person.
    pub message: String,
    /// Whether running the same command again, unchanged, can succeed.
    pub retryable: bool,
    /// Facts about the failure, such as the argument, the value, or the conflicting revision.
    pub details: Value,
    /// Commands or changes that recover from the failure, most useful first.
    pub suggestions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_carries_every_contract_field() {
        let document = Failure::conflict("taken")
            .with_details(json!({ "name": "groceries" }))
            .suggest("mcp notes get groceries")
            .to_json();
        assert_eq!(document["error"]["code"], "CONFLICT");
        assert_eq!(document["error"]["retryable"], false);
        assert_eq!(document["error"]["details"]["name"], "groceries");
        assert_eq!(document["error"]["suggestions"][0], "mcp notes get groceries");
    }

    #[test]
    fn only_a_busy_store_is_retryable() {
        let retryable: Vec<ErrorCode> =
            ErrorCode::ALL.into_iter().filter(|code| code.retryable()).collect();
        assert_eq!(retryable, [ErrorCode::Unavailable]);
    }
}
