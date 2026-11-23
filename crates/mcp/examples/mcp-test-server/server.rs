//! The MCP side of the test server: what `initialize` advertises, `tools/list` in pages, and
//! `tools/call`.

use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestMethod, CallToolRequestParams, CallToolResponse, Implementation,
        ListToolsRequestMethod, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
        ServerConfig,
    },
    service::{RequestContext, RoleServer},
};

use crate::tools::{self, TOOLS};

/// Tools per `tools/list` page, fewer than the server has so clients must follow `nextCursor`.
const PAGE_SIZE: usize = 4;
/// A cursor is this prefix and the index of the page's first tool; clients treat it as opaque.
const CURSOR_PREFIX: &str = "after:";

#[derive(Clone)]
pub(crate) struct TestServer {
    /// Whether the server declares the `tools` capability.
    tools: bool,
    instructions: String,
}

impl TestServer {
    pub(crate) fn new(tools: bool, instructions: String) -> Self {
        Self { tools, instructions }
    }
}

impl ServerHandler for TestServer {
    fn get_info(&self) -> ServerConfig {
        let capabilities = if self.tools {
            ServerCapabilities::builder().enable_tools().build()
        } else {
            ServerCapabilities::default()
        };
        ServerConfig::new(capabilities)
            .with_server_info(Implementation::new("mcp-test-server", env!("CARGO_PKG_VERSION")))
            .with_instructions(self.instructions.clone())
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        if !self.tools {
            return Err(ErrorData::method_not_found::<ListToolsRequestMethod>());
        }
        let start = match request.and_then(|request| request.cursor) {
            None => 0,
            Some(cursor) => cursor
                .strip_prefix(CURSOR_PREFIX)
                .and_then(|start| start.parse::<usize>().ok())
                .filter(|&start| 0 < start && start < TOOLS.len())
                .ok_or_else(|| {
                    ErrorData::invalid_params(format!("invalid cursor {cursor}"), None)
                })?,
        };
        let end = TOOLS.len().min(start + PAGE_SIZE);
        let mut result = ListToolsResult::with_all_items(TOOLS[start..end].to_vec());
        if end < TOOLS.len() {
            result.next_cursor = Some(format!("{CURSOR_PREFIX}{end}"));
        }
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !self.tools {
            return Err(ErrorData::method_not_found::<CallToolRequestMethod>());
        }
        let arguments = request.arguments.unwrap_or_default();
        tools::call(&request.name, arguments, &context).await.map(CallToolResponse::from)
    }
}
