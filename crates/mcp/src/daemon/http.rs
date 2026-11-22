//! Starting a session with a Streamable HTTP server.

use std::collections::{BTreeMap, HashMap};

use reqwest::header::{HeaderName, HeaderValue};
use rmcp::service::{RoleClient, RunningService, serve_client};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde_json::json;

use super::failures::start_failure;
use super::handler::Client;
use crate::failure::Failure;

/// Connects to `url`, sending `headers` with every request, and completes the MCP handshake.
pub(crate) async fn start(
    server: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
    client: Client,
) -> Result<RunningService<RoleClient, Client>, Failure> {
    let config = StreamableHttpClientTransportConfig::with_uri(url)
        .custom_headers(header_map(server, headers)?);
    let transport = StreamableHttpClientTransport::from_config(config);
    serve_client(client, transport).await.map_err(|error| start_failure(server, error))
}

/// The configured headers as HTTP headers. A failure names the header, never its value.
fn header_map(
    server: &str,
    headers: &BTreeMap<String, String>,
) -> Result<HashMap<HeaderName, HeaderValue>, Failure> {
    headers
        .iter()
        .map(|(name, value)| {
            let invalid = |what: &str| {
                Failure::precondition(format!("server `{server}`'s header `{name}` has {what}"))
                    .with_details(json!({ "server": server, "header": name }))
            };
            let header_name =
                HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid("an invalid name"))?;
            let header_value =
                HeaderValue::from_str(value).map_err(|_| invalid("an invalid value"))?;
            Ok((header_name, header_value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::failure::ErrorCode;

    use super::*;

    fn headers(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
    }

    #[test]
    fn valid_headers_pass_through() -> Result<(), Failure> {
        let map = header_map("remote", &headers(&[("Authorization", "Bearer token")]))?;
        let authorization = map.get(&HeaderName::from_static("authorization"));
        assert_eq!(authorization.map(HeaderValue::as_bytes), Some(&b"Bearer token"[..]));
        Ok(())
    }

    #[test]
    fn an_invalid_value_is_named_but_not_shown() {
        let secret = "Bearer secret\nvalue";
        let Err(failure) = header_map("remote", &headers(&[("Authorization", secret)])) else {
            panic!("a value with a newline was accepted");
        };
        assert_eq!(failure.code, ErrorCode::PreconditionFailed);
        assert!(failure.message.contains("Authorization"), "{}", failure.message);
        assert!(!failure.to_json().to_string().contains("secret"), "{}", failure.message);
    }

    #[test]
    fn an_invalid_name_is_refused() {
        let failure = header_map("remote", &headers(&[("Bad Header", "x")])).err();
        assert_eq!(failure.map(|failure| failure.code), Some(ErrorCode::PreconditionFailed));
    }
}
