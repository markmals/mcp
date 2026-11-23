//! What the daemon tells servers about itself, and what it does with what servers send it.

use std::sync::{Arc, Mutex, PoisonError};

use rmcp::ClientHandler;
use rmcp::model::{
    ClientCapabilities, ClientConfig, ElicitRequestParams, ElicitResult,
    ElicitationCreateRequestMethod, ErrorData, Implementation, ListRootsRequestMethod,
    ProtocolVersion,
};
use rmcp::service::{NotificationContext, RequestContext, RoleClient};
use serde_json::Value;

/// A session's tool list, emptied when the server announces that its tools changed.
#[derive(Debug, Default)]
pub(crate) struct ToolCache(Mutex<CachedTools>);

#[derive(Debug, Default)]
struct CachedTools {
    /// Counts announcements, so a list fetched across one is not stored as current.
    generation: u64,
    tools: Option<Vec<Value>>,
}

impl ToolCache {
    pub(crate) fn tools(&self) -> Option<Vec<Value>> {
        self.lock().tools.clone()
    }

    pub(crate) fn generation(&self) -> u64 {
        self.lock().generation
    }

    /// Stores `tools` unless the server announced a change since `generation` was read.
    pub(crate) fn store(&self, generation: u64, tools: &[Value]) {
        let mut cached = self.lock();
        if cached.generation == generation {
            cached.tools = Some(tools.to_vec());
        }
    }

    fn invalidate(&self) {
        let mut cached = self.lock();
        cached.generation += 1;
        cached.tools = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CachedTools> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The client side of one session. It declares no capabilities, so it refuses every request a
/// server could make of a client except `ping`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Client {
    pub(crate) tools: Arc<ToolCache>,
}

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("mcp", env!("CARGO_PKG_VERSION")),
        )
        .with_protocol_version(ProtocolVersion::LATEST_WITH_INITIALIZE)
    }

    // rmcp answers `roots/list` with an empty list by default; a client without the roots
    // capability must refuse it instead.
    #[expect(deprecated, reason = "roots are deprecated by SEP-2577 but servers can still ask")]
    async fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> Result<rmcp::model::ListRootsResult, ErrorData> {
        Err(ErrorData::method_not_found::<ListRootsRequestMethod>())
    }

    // rmcp declines elicitation by default; a client without the capability refuses it.
    async fn create_elicitation(
        &self,
        _request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, ErrorData> {
        Err(ErrorData::method_not_found::<ElicitationCreateRequestMethod>())
    }

    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        self.tools.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rmcp::model::ServerRequest;
    use rmcp::{ServerHandler, ServiceError, ServiceExt};
    use serde_json::json;

    use super::*;

    /// A server that only asks things of its client.
    struct Probe;

    impl ServerHandler for Probe {}

    #[test]
    fn a_list_fetched_across_an_announcement_is_not_kept() {
        let cache = ToolCache::default();
        let generation = cache.generation();
        cache.invalidate();
        cache.store(generation, &[json!({"name": "stale"})]);
        assert_eq!(cache.tools(), None);

        cache.store(cache.generation(), &[json!({"name": "fresh"})]);
        assert_eq!(cache.tools(), Some(vec![json!({"name": "fresh"})]));
    }

    #[test]
    fn declares_no_capabilities() -> Result<(), serde_json::Error> {
        let info = serde_json::to_value(Client::default().get_info())?;
        assert_eq!(info["capabilities"], json!({}));
        assert_eq!(info["clientInfo"]["name"], "mcp");
        Ok(())
    }

    #[tokio::test]
    async fn server_requests_are_refused_and_tool_announcements_empty_the_cache()
    -> Result<(), Box<dyn std::error::Error>> {
        let (client_end, server_end) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(Probe.serve(server_end));
        let client = Client::default();
        let tools = Arc::clone(&client.tools);
        let _client = client.serve(client_end).await?;
        let server = server.await??;

        let requests = [
            json!({"method": "roots/list"}),
            json!({"method": "sampling/createMessage", "params": {"messages": [], "maxTokens": 1}}),
            json!({"method": "elicitation/create", "params": {
                "mode": "form", "message": "name?",
                "requestedSchema": {"type": "object", "properties": {}},
            }}),
        ];
        for request in requests {
            let method = request["method"].clone();
            match server.send_request(serde_json::from_value::<ServerRequest>(request)?).await {
                Err(ServiceError::McpError(error)) => {
                    assert_eq!(error.code, rmcp::model::ErrorCode::METHOD_NOT_FOUND, "{method}");
                }
                other => panic!("{method} was answered with {other:?}"),
            }
        }

        tools.store(tools.generation(), &[json!({"name": "old"})]);
        server.notify_tool_list_changed().await?;
        for _ in 0..100 {
            if tools.tools().is_none() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Err("the announcement did not empty the cache".into())
    }
}
