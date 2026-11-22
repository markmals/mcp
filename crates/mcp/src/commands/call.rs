//! `mcp call`: calling a server's tool, its input properties given as flags.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::NAME;
use crate::commands::approval::ReceiptStatus;
use crate::commands::tools::{SERVER, TOOL_NAME};
use crate::connect::Connector;
use crate::daemon::protocol::SessionInfo;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{
    ArgumentKind, ArgumentSpec, CommandSpec, DRY_RUN, Effect, Example, Requirement, TIMEOUT,
    ValueKind, shell_word,
};
use crate::response::{RenderText, Response};
use crate::tool_flags::{FlagInput, FlagType, ToolFlag, flags, parse};
use crate::tool_input::{InputValidation, base_object, validate};

const INPUT: ArgumentSpec = ArgumentSpec {
    name: "input",
    kind: ArgumentKind::Option,
    value: ValueKind::Text,
    required: false,
    default: None,
    multiple: false,
    summary: "The tool's input as a JSON object, `@<file>`, or `-` for stdin; flags after the \
              tool set or replace its properties",
};

const TOOL_ARGUMENTS: ArgumentSpec = ArgumentSpec {
    name: "arguments",
    kind: ArgumentKind::Trailing,
    value: ValueKind::String,
    required: false,
    default: None,
    multiple: true,
    summary: "The tool's input properties as flags, such as `--url https://example.com`; \
              `mcp tools info <server> <tool>` lists them, and `--help` here prints them",
};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "call",
    path: &["call"],
    summary: "Call a server's tool with its input properties as flags, and print its result",
    description: "Starts or reuses the server's session, reads the tool's inputSchema, builds \
        the input from --input (default `{}`) with each flag after the tool setting one property \
        (`--file-path x` or `--filePath x` for `filePath`), checks it against the schema, and \
        calls the tool. mcp's own options go before the server: every word after the tool \
        belongs to the tool. The result is the server's CallToolResult unchanged, with the \
        session that answered; `--output text` prints its text content. A tool that reports an \
        error (isError) fails with TOOL_ERROR and its whole result in `details.result`. \
        --dry-run checks the input and shows what would be sent without calling the tool; the \
        tool's own validation and effects stay unknown until it runs. `--help` after the tool \
        prints its flags.",
    arguments: &[INPUT, DRY_RUN, TIMEOUT, SERVER, TOOL_NAME, TOOL_ARGUMENTS],
    effect: Effect::Calls,
    requires: &[Requirement::Configuration, Requirement::Servers],
    dry_run: true,
    idempotent: false,
    errors: &[
        ErrorCode::InvalidArgument,
        ErrorCode::NotFound,
        ErrorCode::PreconditionFailed,
        ErrorCode::Unauthenticated,
        ErrorCode::Unavailable,
        ErrorCode::Timeout,
        ErrorCode::ToolError,
        ErrorCode::ServerError,
    ],
    examples: &[
        Example {
            summary: "Call `echo` on the server `demo` with a text property",
            args: &["call", "demo", "echo", "--text", "hello"],
        },
        Example {
            summary: "Check the input and show what would be sent, without calling the tool",
            args: &["call", "--dry-run", "demo", "echo", "--text", "hello", "--count", "2"],
        },
        Example {
            summary: "Pass the input as JSON",
            args: &["call", "--input", "{\"text\":\"hello\"}", "demo", "echo"],
        },
    ],
    synonyms: &["run", "invoke", "execute", "use", "tool", "request"],
    output_schema: || schema_for!(CallResult),
    run: call,
}];

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CallResult {
    /// `completed`, or `dry-run` when the tool was not called.
    pub status: ReceiptStatus,
    pub server: String,
    pub tool: String,
    pub session: SessionInfo,
    pub input_validation: InputValidation,
    /// The input that would be sent; present only for `dry-run`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Map<String, Value>>,
    /// The server's CallToolResult, unchanged: `content`, `structuredContent`, `isError`. Null
    /// for `dry-run`.
    pub result: Option<Value>,
    /// What a dry run cannot tell.
    pub caveats: Vec<String>,
}

impl RenderText for CallResult {
    fn render_text(&self) -> String {
        match &self.result {
            Some(result) => content_text(result),
            None => format!(
                "would call {} {} with {}\n",
                self.server,
                self.tool,
                Value::Object(self.arguments.clone().unwrap_or_default())
            ),
        }
    }
}

/// The text content blocks, and one line for each other block.
fn content_text(result: &Value) -> String {
    let blocks = result.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut text = String::new();
    for block in &blocks {
        let kind = block["type"].as_str().unwrap_or("unknown");
        let line = match kind {
            "text" => block["text"].as_str().unwrap_or_default().to_string(),
            "image" | "audio" => {
                let bytes = block["data"].as_str().map_or(0, |data| data.len() / 4 * 3);
                format!(
                    "[{kind} {}, about {bytes} bytes]",
                    block["mimeType"].as_str().unwrap_or("?")
                )
            }
            "resource_link" => format!("[resource link {}]", block["uri"].as_str().unwrap_or("?")),
            "resource" => format!(
                "[resource {} {}]",
                block["resource"]["uri"].as_str().unwrap_or("?"),
                block["resource"]["mimeType"].as_str().unwrap_or("")
            ),
            other => format!("[{other} content]"),
        };
        text.push_str(line.trim_end_matches('\n'));
        text.push('\n');
    }
    text
}

fn call(invocation: &Invocation) -> Result<Response, Failure> {
    let connector = Connector::load(invocation)?;
    let server =
        connector.configuration.server(&invocation.string("server").unwrap_or_default())?;
    let tool_name = invocation.string("tool").unwrap_or_default();
    let label = format!("{} {}", shell_word(&server.name), shell_word(&tool_name));
    let (session, tool) = connector.tool(server, &tool_name)?;
    let schema = tool.get("inputSchema").cloned().unwrap_or_else(|| json!({}));
    let mut arguments = base_object(invocation.text("input")?.as_deref())?;
    match parse(&schema, &invocation.strings("arguments"), &label)? {
        FlagInput::Help => return Ok(Response::usage(usage(&server.name, &tool, &flags(&schema)))),
        FlagInput::Arguments(flagged) => arguments.extend(flagged),
    }
    let input_validation = validate(&schema, &arguments, &label)?;
    if invocation.dry_run() {
        return Ok(Response::new(&CallResult {
            status: ReceiptStatus::DryRun,
            server: server.name.clone(),
            tool: tool_name,
            session,
            input_validation,
            arguments: Some(arguments),
            result: None,
            caveats: vec![
                "The tool was not called; the server's own validation of the input and the \
                 tool's effects are unknown until it runs."
                    .into(),
            ],
        }));
    }
    let (mut called, result) = connector.call(server, &tool_name, arguments)?;
    // Looking the tool up may have started the session; "reused" is about earlier commands.
    called.reused = session.reused && called.id == session.id;
    let session = called;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(tool_error(&server.name, &tool_name, result));
    }
    Ok(Response::new(&CallResult {
        status: ReceiptStatus::Completed,
        server: server.name.clone(),
        tool: tool_name,
        session,
        input_validation,
        arguments: None,
        result: Some(result),
        caveats: Vec::new(),
    }))
}

fn tool_error(server: &str, tool: &str, result: Value) -> Failure {
    let first: String =
        content_text(&result).lines().next().unwrap_or_default().chars().take(200).collect();
    Failure::new(
        ErrorCode::ToolError,
        format!("tool `{tool}` on `{server}` reported an error: {first}"),
    )
    .with_details(json!({ "server": server, "tool": tool, "result": result }))
    .suggest(format!("{NAME} tools info {} {}", shell_word(server), shell_word(tool)))
}

/// The tool's usage, as `mcp call <server> <tool> --help` and `tools info --output text`
/// print it.
pub fn usage(server: &str, tool: &Value, flags: &[ToolFlag]) -> String {
    let name = tool["name"].as_str().unwrap_or_default();
    let mut text = format!("{}\n\n", example(server, name, flags));
    if let Some(description) = tool["description"].as_str() {
        text.push_str(description.trim());
        text.push_str("\n\n");
    }
    if flags.is_empty() {
        text.push_str("This tool takes no input.\n");
    }
    for flag in flags {
        let kind = match (flag.flag_type, flag.item_type) {
            (FlagType::Array, Some(item)) => format!("{}, repeatable", type_name(item)),
            (other, _) => type_name(other).to_string(),
        };
        let required = if flag.required { ", required" } else { "" };
        text.push_str(&format!("  {} <{kind}>{required}\n", flag.flag));
        if let Some(description) = &flag.description {
            text.push_str(&format!("      {}\n", description.lines().next().unwrap_or_default()));
        }
        if let Some(values) = &flag.accepted_values {
            let values: Vec<String> = values.iter().map(Value::to_string).collect();
            text.push_str(&format!("      one of {}\n", values.join(", ")));
        }
    }
    text
}

fn type_name(flag_type: FlagType) -> &'static str {
    match flag_type {
        FlagType::String => "string",
        FlagType::Integer => "integer",
        FlagType::Number => "number",
        FlagType::Boolean => "true|false",
        FlagType::Array => "array",
        FlagType::Json => "json",
    }
}

/// An `mcp call` command line with every required flag and a placeholder for its value.
pub fn example(server: &str, tool: &str, flags: &[ToolFlag]) -> String {
    let mut words = vec![NAME.to_string(), "call".into(), shell_word(server), shell_word(tool)];
    for flag in flags.iter().filter(|flag| flag.required) {
        words.push(flag.flag.clone());
        if flag.flag_type != FlagType::Boolean {
            words.push(format!("<{}>", flag.property));
        }
    }
    words.join(" ")
}
