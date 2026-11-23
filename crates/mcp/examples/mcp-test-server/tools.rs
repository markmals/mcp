//! The test server's tools: their definitions, written as the JSON `tools/list` sends, and what
//! each does when called.

use std::{
    io::{self, Write},
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use rmcp::{
    ErrorData,
    model::{CallToolResult, ContentBlock, ErrorCode, JsonObject, Tool},
    service::{RequestContext, RoleServer},
};
use serde_json::{Value, json};

/// Every tool, in the order `tools/list` declares them.
pub(crate) static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(|| {
    vec![
        tool(json!({
            "name": "echo",
            "title": "Echo",
            "description": "Echoes its arguments back.\n\nA second paragraph that tools list must not show.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "Text to echo"},
                    "count": {"type": "integer"},
                    "ratio": {"type": "number"},
                    "loud": {"type": "boolean"},
                    "tags": {"type": "array", "items": {"type": "string"}},
                    "options": {"type": "object"},
                    "mode": {"enum": ["plain", "fancy"]},
                    "filePath": {"type": "string"},
                    "maybe": {"type": ["string", "null"]}
                },
                "required": ["text"]
            },
            "outputSchema": {
                "type": "object",
                "properties": {"arguments": {"type": "object"}},
                "required": ["arguments"]
            },
            "annotations": {"readOnlyHint": true, "idempotentHint": true}
        })),
        tool(json!({
            "name": "fail",
            "description": "Fails with the given reason as a tool error.",
            "inputSchema": {"type": "object", "properties": {"reason": {"type": "string"}}}
        })),
        tool(json!({
            "name": "counter",
            "description": "Counts its calls in this server process.",
            "inputSchema": {"type": "object", "properties": {}}
        })),
        tool(json!({
            "name": "slow",
            "description": "Waits the given number of seconds, then answers.",
            "inputSchema": {
                "type": "object",
                "properties": {"seconds": {"type": "number"}},
                "required": ["seconds"]
            }
        })),
        tool(json!({
            "name": "environment",
            "description": "Reads one of the server's environment variables and its working directory.",
            "inputSchema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }
        })),
        tool(json!({
            "name": "log",
            "description": "Writes a line to the server's standard error.",
            "inputSchema": {
                "type": "object",
                "properties": {"line": {"type": "string"}},
                "required": ["line"]
            }
        })),
        tool(json!({
            "name": "collide",
            "description": "Echoes arguments named like the client's own flags.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "output": {"type": "string"},
                    "input": {"type": "string"},
                    "help": {"type": "string"},
                    "file_path": {"type": "string"},
                    "filePath": {"type": "string"}
                }
            }
        })),
        tool(json!({
            "name": "broken_schema",
            "description": "Echoes its arguments; its input schema cannot be compiled.",
            "inputSchema": {"type": "object", "properties": {"x": {"$ref": "#/$defs/missing"}}}
        })),
        tool(json!({
            "name": "protocol_error",
            "description": "Answers with a JSON-RPC internal error instead of a result.",
            "inputSchema": {"type": "object", "properties": {}}
        })),
        tool(json!({
            "name": "invalid_params",
            "description": "Answers with a JSON-RPC invalid-params error instead of a result.",
            "inputSchema": {"type": "object", "properties": {}}
        })),
    ]
});

/// Calls to `counter` in this process.
static CALLS: AtomicU64 = AtomicU64::new(0);

fn tool(definition: Value) -> Tool {
    serde_json::from_value(definition).expect("the tool definitions above are valid tools")
}

/// Runs tool `name`. A protocol failure is `Err`, which rmcp sends as the JSON-RPC error.
pub(crate) async fn call(
    name: &str,
    arguments: JsonObject,
    context: &RequestContext<RoleServer>,
) -> Result<CallToolResult, ErrorData> {
    match name {
        "echo" | "collide" | "broken_schema" => Ok(echo(arguments)),
        "fail" => Ok(fail(&arguments)),
        "counter" => Ok(counter()),
        "slow" => slow(&arguments, context).await,
        "environment" => environment(&arguments),
        "log" => log(&arguments),
        "protocol_error" => Err(ErrorData::new(
            ErrorCode::INTERNAL_ERROR,
            "deliberate protocol error",
            Some(json!({"hint": "test"})),
        )),
        "invalid_params" => Err(ErrorData::invalid_params("bad params", None)),
        _ => Err(ErrorData::invalid_params(format!("unknown tool: {name}"), None)),
    }
}

fn echo(arguments: JsonObject) -> CallToolResult {
    let arguments = Value::Object(arguments);
    let mut result = CallToolResult::success(vec![ContentBlock::text(arguments.to_string())]);
    result.structured_content = Some(json!({"arguments": arguments}));
    result
}

fn fail(arguments: &JsonObject) -> CallToolResult {
    let reason = arguments.get("reason").and_then(Value::as_str).unwrap_or("no reason given");
    CallToolResult::error(vec![ContentBlock::text(format!("failed: {reason}"))])
}

fn counter() -> CallToolResult {
    let count = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    let mut result = CallToolResult::success(vec![ContentBlock::text(count.to_string())]);
    result.structured_content = Some(json!({"count": count, "pid": std::process::id()}));
    result
}

async fn slow(
    arguments: &JsonObject,
    context: &RequestContext<RoleServer>,
) -> Result<CallToolResult, ErrorData> {
    let seconds = arguments
        .get("seconds")
        .and_then(Value::as_f64)
        .ok_or_else(|| ErrorData::invalid_params("`seconds` must be a number", None))?;
    let duration = Duration::try_from_secs_f64(seconds)
        .map_err(|error| ErrorData::invalid_params(format!("`seconds`: {error}"), None))?;
    // rmcp cancels `context.ct` on `notifications/cancelled` or a closed HTTP stream, but leaves
    // the handler running; stopping is the handler's job.
    tokio::select! {
        () = tokio::time::sleep(duration) => {
            Ok(CallToolResult::success(vec![ContentBlock::text("done")]))
        }
        () = context.ct.cancelled() => Err(ErrorData::internal_error("cancelled", None)),
    }
}

fn environment(arguments: &JsonObject) -> Result<CallToolResult, ErrorData> {
    let name = required_string(arguments, "name")?;
    let value = std::env::var_os(name).map(|value| value.to_string_lossy().into_owned());
    let cwd = std::env::current_dir().map_err(|error| {
        ErrorData::internal_error(format!("cannot read the working directory: {error}"), None)
    })?;
    Ok(CallToolResult::structured(json!({"value": value, "cwd": cwd.to_string_lossy()})))
}

fn log(arguments: &JsonObject) -> Result<CallToolResult, ErrorData> {
    let line = required_string(arguments, "line")?;
    writeln!(io::stderr(), "{line}").map_err(|error| {
        ErrorData::internal_error(format!("cannot write to stderr: {error}"), None)
    })?;
    Ok(CallToolResult::success(vec![ContentBlock::text("logged")]))
}

fn required_string<'a>(arguments: &'a JsonObject, name: &str) -> Result<&'a str, ErrorData> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ErrorData::invalid_params(format!("`{name}` must be a string"), None))
}
