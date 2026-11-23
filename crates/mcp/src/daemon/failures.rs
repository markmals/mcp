//! How what goes wrong between the daemon and a server becomes a [`Failure`].

use std::error::Error;
use std::time::Duration;

use reqwest::StatusCode;
use rmcp::ServiceError;
use rmcp::model::ErrorData;
use rmcp::service::ClientInitializeError;
use rmcp::transport::streamable_http_client::StreamableHttpError;
use serde_json::{Value, json};

use crate::failure::{ErrorCode, Failure};

type HttpError = StreamableHttpError<reqwest::Error>;

/// Why a session could not start.
pub(crate) fn start_failure(server: &str, error: ClientInitializeError) -> Failure {
    match error {
        ClientInitializeError::JsonRpcError(data) => protocol_failure(server, &data),
        ClientInitializeError::TransportError { error, .. } => {
            transport_failure(server, error.error.as_ref())
        }
        ClientInitializeError::LegacyFallbackFailed { fallback, .. } => {
            start_failure(server, *fallback)
        }
        ClientInitializeError::ConnectionClosed(_) => Failure::unavailable(format!(
            "server `{server}` closed the connection before it finished starting"
        )),
        other => Failure::unavailable(format!(
            "server `{server}` did not start a session: {}",
            describe(&other)
        )),
    }
}

/// Why a request on a running session failed.
pub(crate) fn request_failure(server: &str, error: ServiceError) -> Failure {
    match error {
        ServiceError::McpError(data) => protocol_failure(server, &data),
        ServiceError::TransportSend(error) => transport_failure(server, error.error.as_ref()),
        ServiceError::TransportClosed => connection_lost(server),
        ServiceError::Timeout { timeout } => timed_out(server, timeout),
        other => Failure::new(
            ErrorCode::ServerError,
            format!("server `{server}` answered unexpectedly: {}", describe(&other)),
        ),
    }
}

pub(crate) fn connection_lost(server: &str) -> Failure {
    Failure::unavailable(format!("the connection to server `{server}` was lost"))
}

pub(crate) fn timed_out(server: &str, limit: Duration) -> Failure {
    Failure::new(
        ErrorCode::Timeout,
        format!("server `{server}` did not answer within {}", describe_duration(limit)),
    )
}

/// `1.5 seconds`, `120 seconds`, `250 milliseconds`.
pub(crate) fn describe_duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{} milliseconds", duration.as_millis())
    } else if duration.subsec_millis() == 0 {
        format!("{} seconds", duration.as_secs())
    } else {
        format!("{:.1} seconds", duration.as_secs_f64())
    }
}

/// Adds `server` and the server's captured stderr to a failure's details.
pub(crate) fn with_stderr(failure: Failure, server: &str, stderr: Vec<String>) -> Failure {
    let mut details = match *failure.details {
        Value::Object(details) => details,
        _ => serde_json::Map::new(),
    };
    details.insert("server".to_owned(), Value::from(server));
    details.insert("stderr".to_owned(), Value::from(stderr));
    Failure { details: Box::new(Value::Object(details)), ..failure }
}

fn protocol_failure(server: &str, data: &ErrorData) -> Failure {
    let details = json!({ "code": data.code.0, "message": data.message, "data": data.data });
    if data.code == rmcp::model::ErrorCode::INVALID_PARAMS {
        Failure::invalid_argument(format!(
            "server `{server}` rejected the request's parameters: {}",
            data.message
        ))
        .with_details(details)
    } else {
        Failure::new(ErrorCode::ServerError, format!("server `{server}`: {}", data.message))
            .with_details(details)
    }
}

fn transport_failure(server: &str, error: &(dyn Error + Send + Sync + 'static)) -> Failure {
    match error.downcast_ref::<HttpError>() {
        Some(error) => http_failure(server, error),
        None => Failure::unavailable(format!(
            "the connection to server `{server}` failed: {}",
            describe(error)
        )),
    }
}

fn http_failure(server: &str, error: &HttpError) -> Failure {
    match error {
        HttpError::AuthRequired(_) => unauthenticated(server, StatusCode::UNAUTHORIZED),
        HttpError::InsufficientScope(_) => unauthenticated(server, StatusCode::FORBIDDEN),
        HttpError::UnexpectedServerResponse(message) if message.starts_with("HTTP 401") => {
            unauthenticated(server, StatusCode::UNAUTHORIZED)
        }
        HttpError::UnexpectedServerResponse(message) if message.starts_with("HTTP 403") => {
            unauthenticated(server, StatusCode::FORBIDDEN)
        }
        HttpError::HttpStatus(status) => {
            Failure::unavailable(format!("server `{server}` answered HTTP {}", status.status))
                .with_details(json!({ "status": status.status, "retryAfter": status.retry_after }))
        }
        HttpError::ReservedHeaderConflict(name) => Failure::precondition(format!(
            "server `{server}` configures header `{name}`, which MCP's HTTP transport sets itself"
        ))
        .with_details(json!({ "server": server, "header": name })),
        HttpError::Client(error) if error.is_connect() => Failure::unavailable(format!(
            "cannot connect to server `{server}`: {}",
            describe(error)
        )),
        other => {
            Failure::unavailable(format!("server `{server}` failed over HTTP: {}", describe(other)))
        }
    }
}

fn unauthenticated(server: &str, status: StatusCode) -> Failure {
    Failure::new(
        ErrorCode::Unauthenticated,
        format!(
            "server `{server}` refused the request's credentials (HTTP {}); mcp does not sign in \
             to servers with OAuth",
            status.as_u16()
        ),
    )
    .with_details(json!({ "status": status.as_u16() }))
    .suggest(
        "set the server's `headers` to credentials it accepts, such as an Authorization header",
    )
}

/// An error and its sources, skipping a source its parent's message already includes.
fn describe(error: &(dyn Error + 'static)) -> String {
    let mut description = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let message = cause.to_string();
        if !description.contains(&message) {
            description = format!("{description}: {message}");
        }
        source = cause.source();
    }
    description
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use rmcp::transport::streamable_http_client::{AuthRequiredError, HttpStatusError};

    use super::*;

    fn http(error: HttpError) -> Failure {
        http_failure("remote", &error)
    }

    #[test]
    fn invalid_params_is_an_invalid_argument() {
        let data = ErrorData::invalid_params("missing `text`", Some(json!({"field": "text"})));
        let failure = request_failure("echo", ServiceError::McpError(data));
        assert_eq!(failure.code, ErrorCode::InvalidArgument);
        assert_eq!(failure.details["data"], json!({"field": "text"}));
    }

    #[test]
    fn other_json_rpc_errors_are_server_errors_with_code_message_and_data() {
        let data = ErrorData::internal_error("boom", None);
        let failure = request_failure("echo", ServiceError::McpError(data));
        assert_eq!(failure.code, ErrorCode::ServerError);
        assert_eq!(failure.message, "server `echo`: boom");
        assert_eq!(*failure.details, json!({"code": -32603, "message": "boom", "data": null}));
    }

    #[test]
    fn refused_credentials_are_unauthenticated() {
        let challenge = http(HttpError::AuthRequired(AuthRequiredError::new("Bearer".into())));
        assert_eq!(challenge.code, ErrorCode::Unauthenticated);
        let forbidden =
            http(HttpError::UnexpectedServerResponse(Cow::from("HTTP 403 Forbidden: no")));
        assert_eq!(forbidden.code, ErrorCode::Unauthenticated);
        assert!(forbidden.message.contains("OAuth"), "{}", forbidden.message);
    }

    #[test]
    fn rate_limits_and_server_failures_are_unavailable() {
        let failure = http(HttpError::HttpStatus(HttpStatusError::new(503, Some("5".into()))));
        assert_eq!(failure.code, ErrorCode::Unavailable);
        assert_eq!(failure.details["retryAfter"], "5");
    }

    #[test]
    fn stderr_joins_the_details() {
        let failure = with_stderr(connection_lost("echo"), "echo", vec!["panic".to_owned()]);
        assert_eq!(*failure.details, json!({"server": "echo", "stderr": ["panic"]}));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(describe_duration(Duration::from_secs(120)), "120 seconds");
        assert_eq!(describe_duration(Duration::from_millis(1500)), "1.5 seconds");
        assert_eq!(describe_duration(Duration::from_millis(250)), "250 milliseconds");
    }
}
