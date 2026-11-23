//! The structured error every failing command reports on stderr.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A stable error code. Codes, exit statuses, and retryability are part of the CLI contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Input is missing or malformed.
    InvalidArgument,
    /// A named server, tool, command, or document does not exist.
    NotFound,
    /// The configuration, the server, or its approval is not ready for the operation.
    PreconditionFailed,
    /// The person declined approval or could not be asked.
    PermissionDenied,
    /// An HTTP server refused the request's credentials.
    Unauthenticated,
    /// A server or the daemon could not be reached; retrying can succeed.
    Unavailable,
    /// No answer within `--timeout`.
    Timeout,
    /// The tool ran and reported an error.
    ToolError,
    /// The server answered with a protocol error.
    ServerError,
    /// A defect in `mcp` itself, or a stdout that cannot take the result.
    Internal,
}

impl ErrorCode {
    /// Every code, in the order the reference lists them.
    pub const ALL: [ErrorCode; 10] = [
        ErrorCode::InvalidArgument,
        ErrorCode::NotFound,
        ErrorCode::PreconditionFailed,
        ErrorCode::PermissionDenied,
        ErrorCode::Unauthenticated,
        ErrorCode::Unavailable,
        ErrorCode::Timeout,
        ErrorCode::ToolError,
        ErrorCode::ServerError,
        ErrorCode::Internal,
    ];

    /// The code as it appears in the error JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidArgument => "INVALID_ARGUMENT",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::PreconditionFailed => "PRECONDITION_FAILED",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::Unauthenticated => "UNAUTHENTICATED",
            ErrorCode::Unavailable => "UNAVAILABLE",
            ErrorCode::Timeout => "TIMEOUT",
            ErrorCode::ToolError => "TOOL_ERROR",
            ErrorCode::ServerError => "SERVER_ERROR",
            ErrorCode::Internal => "INTERNAL",
        }
    }

    /// The process exit status the code sets. Success is always 0.
    pub fn exit_status(self) -> u8 {
        match self {
            ErrorCode::InvalidArgument => 2,
            ErrorCode::NotFound
            | ErrorCode::PreconditionFailed
            | ErrorCode::PermissionDenied
            | ErrorCode::Unauthenticated => 3,
            ErrorCode::Unavailable | ErrorCode::Timeout => 5,
            ErrorCode::ToolError | ErrorCode::ServerError => 6,
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
                "Input is missing or malformed: an argument, a tool flag, tool input that fails \
                 the tool's inputSchema, or parameters the server rejected; `details` says which."
            }
            ErrorCode::NotFound => "A named server, tool, command, or document does not exist.",
            ErrorCode::PreconditionFailed => {
                "A configuration file is invalid, the server's entry is unusable, a variable it \
                 needs is unset, MCP_REQUIRE_APPROVAL is invalid, approval is required and \
                 missing, or the server has no tools."
            }
            ErrorCode::PermissionDenied => {
                "The person declined to approve, could not be authenticated, or could not be asked."
            }
            ErrorCode::Unauthenticated => {
                "An HTTP server answered 401 or 403; mcp does not sign in to servers with OAuth."
            }
            ErrorCode::Unavailable => {
                "A server could not be started or reached, the connection was lost, the server \
                 answered 429 or 5xx, or the daemon or the approvals file is busy; retry the same \
                 command."
            }
            ErrorCode::Timeout => {
                "No answer within --timeout. A tool call may still have run: verify its effect \
                 before retrying, unless the tool declares readOnlyHint or idempotentHint."
            }
            ErrorCode::ToolError => {
                "The tool ran and reported an error (isError); `details.result` holds its whole \
                 result."
            }
            ErrorCode::ServerError => {
                "The server answered with a protocol error; `details` carries its code, message, \
                 and data."
            }
            ErrorCode::Internal => {
                "A defect in mcp itself, or a stdout that cannot take the result; `message` says \
                 which."
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

    pub fn unavailable(message: impl Into<String>) -> Failure {
        Failure::new(ErrorCode::Unavailable, message)
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
    /// Facts about the failure, such as the argument, the server, or the file.
    pub details: Value,
    /// Commands or changes that recover from the failure, most useful first.
    pub suggestions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_carries_every_contract_field() {
        let document = Failure::not_found("no server `x`")
            .with_details(json!({ "server": "x" }))
            .suggest("mcp servers list")
            .to_json();
        assert_eq!(document["error"]["code"], "NOT_FOUND");
        assert_eq!(document["error"]["retryable"], false);
        assert_eq!(document["error"]["details"]["server"], "x");
        assert_eq!(document["error"]["suggestions"][0], "mcp servers list");
    }

    #[test]
    fn only_an_unavailable_server_or_daemon_is_retryable() {
        let retryable: Vec<ErrorCode> =
            ErrorCode::ALL.into_iter().filter(|code| code.retryable()).collect();
        assert_eq!(retryable, [ErrorCode::Unavailable]);
    }
}
