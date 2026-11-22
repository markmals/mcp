//! `mcp tools list`, `search`, and `info`: the tools servers offer.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use serde_json::{Value, json};

use crate::NAME;
use crate::commands::servers::{CURSOR, SERVER_NAME};
use crate::config::Server;
use crate::connect::Connector;
use crate::daemon::protocol::SessionInfo;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{
    ArgumentKind, ArgumentSpec, CommandSpec, Effect, Example, Requirement, SERVER_FILTER, TIMEOUT,
    ValueKind, shell_word,
};
use crate::response::{RenderText, Response};
use crate::search::{self, Page};
use crate::tool_flags::{ToolFlag, flags, kebab_case};

/// The longest summary `tools list` shows, in characters.
const SUMMARY_LENGTH: usize = 200;

pub const TOOL_NAME: ArgumentSpec = ArgumentSpec {
    name: "tool",
    kind: ArgumentKind::Positional,
    value: ValueKind::String,
    required: true,
    default: None,
    multiple: false,
    summary: "The tool's name, as `mcp tools list` shows it",
};

pub const SERVER: ArgumentSpec =
    ArgumentSpec { name: "server", summary: "The server that offers the tool", ..SERVER_NAME };

const COLLECTION_ERRORS: &[ErrorCode] =
    &[ErrorCode::InvalidArgument, ErrorCode::NotFound, ErrorCode::PreconditionFailed];

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "tools.list",
        path: &["tools", "list"],
        summary: "List the tools every usable server offers, one line each",
        description: "Starts or reuses each server's session and pages through its tools, in \
            server-name order and then each server's own order. Each item has the server, the \
            tool's name and title, a one-line summary (at most 200 characters; \
            `summaryTruncated` says when it was cut), and the annotations the server declared. \
            Servers that cannot answer (unusable, unapproved, failing to start, timing out) do \
            not fail the command: `complete` is false and `unavailable` lists each with its \
            error code and message. Pass --server to ask only some servers.",
        arguments: &[SERVER_FILTER, LIMIT, CURSOR, TIMEOUT],
        effect: Effect::Connects,
        requires: &[Requirement::Configuration, Requirement::Servers],
        dry_run: false,
        idempotent: true,
        errors: COLLECTION_ERRORS,
        examples: &[
            Example { summary: "List every server's tools", args: &["tools", "list"] },
            Example {
                summary: "List one server's tools, five at a time",
                args: &["tools", "list", "--server", "demo", "--limit", "5"],
            },
        ],
        synonyms: &["show", "available", "catalog", "functions", "actions"],
        output_schema: || schema_for!(ToolPage),
        run: list,
    },
    CommandSpec {
        id: "tools.search",
        path: &["tools", "search"],
        summary: "Find the tools that fit a task described in plain words, across servers",
        description: "Ranks every usable server's tools against the query by their names, \
            titles, descriptions, and parameters, locally, and returns the best matches, each \
            with the exact `mcp tools info` command to inspect it. Starts or reuses sessions as \
            `tools list` does, and reports servers that could not answer under `unavailable`.",
        arguments: &[QUERY, SERVER_FILTER, SEARCH_LIMIT, TIMEOUT],
        effect: Effect::Connects,
        requires: &[Requirement::Configuration, Requirement::Servers],
        dry_run: false,
        idempotent: true,
        errors: COLLECTION_ERRORS,
        examples: &[Example {
            summary: "Find a tool that echoes text",
            args: &["tools", "search", "echo some text"],
        }],
        synonyms: &["find", "discover", "which", "lookup", "functions"],
        output_schema: || schema_for!(ToolMatches),
        run: search,
    },
    CommandSpec {
        id: "tools.info",
        path: &["tools", "info"],
        summary: "Show a tool's description, input and output schemas, annotations, and flags",
        description: "Returns the tool exactly as the server declared it (`description`, \
            `inputSchema`, `outputSchema`, `annotations`; annotations such as readOnlyHint are \
            the server's own unverified hints), the `flags` `mcp call` accepts for its input \
            properties, and an example `mcp call` command line.",
        arguments: &[SERVER, TOOL_NAME, TIMEOUT],
        effect: Effect::Connects,
        requires: &[Requirement::Configuration, Requirement::Servers],
        dry_run: false,
        idempotent: true,
        errors: &[
            ErrorCode::InvalidArgument,
            ErrorCode::NotFound,
            ErrorCode::PreconditionFailed,
            ErrorCode::Unauthenticated,
            ErrorCode::Unavailable,
            ErrorCode::Timeout,
        ],
        examples: &[Example {
            summary: "Inspect the `echo` tool of the server `demo`",
            args: &["tools", "info", "demo", "echo"],
        }],
        synonyms: &["get", "inspect", "describe", "schema", "arguments", "parameters", "usage"],
        output_schema: || schema_for!(ToolInfo),
        run: info,
    },
];

const LIMIT: ArgumentSpec = ArgumentSpec {
    name: "limit",
    kind: ArgumentKind::Option,
    value: ValueKind::Integer { min: 1, max: Some(200) },
    required: false,
    default: Some("50"),
    multiple: false,
    summary: "The most tools to return",
};

const SEARCH_LIMIT: ArgumentSpec = ArgumentSpec {
    value: ValueKind::Integer { min: 1, max: Some(50) },
    default: Some("10"),
    summary: "The most matches to return",
    ..LIMIT
};

const QUERY: ArgumentSpec = ArgumentSpec {
    name: "query",
    kind: ArgumentKind::Positional,
    value: ValueKind::String,
    required: true,
    default: None,
    multiple: true,
    summary: "What you want to do, in plain words; several words need no quotes",
};

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolSummary {
    pub server: String,
    pub name: String,
    pub title: Option<String>,
    /// The description's first line, at most 200 characters.
    pub summary: String,
    pub summary_truncated: bool,
    /// The server's own hints, such as readOnlyHint; unverified.
    pub annotations: Option<Value>,
}

/// A server that could not answer.
#[derive(Serialize, JsonSchema)]
pub struct Unavailable {
    pub server: String,
    /// The error code asking it alone would have reported.
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolPage {
    #[serde(flatten)]
    pub page: Page<ToolSummary>,
    /// Whether every server answered.
    pub complete: bool,
    pub unavailable: Vec<Unavailable>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolMatch {
    pub server: String,
    pub tool: String,
    pub title: Option<String>,
    pub summary: String,
    /// Relevance from 0 to 1.
    pub score: f64,
    /// The command that shows the tool's full contract.
    pub command: String,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolMatches {
    pub items: Vec<ToolMatch>,
    /// Whether more tools matched than `--limit` allowed.
    pub has_more: bool,
    pub complete: bool,
    pub unavailable: Vec<Unavailable>,
}

#[derive(Serialize, JsonSchema)]
pub struct ToolInfo {
    pub server: String,
    pub session: SessionInfo,
    /// The tool exactly as the server declared it. Its annotations are the server's own
    /// unverified hints.
    pub tool: Value,
    /// The flags `mcp call` accepts for the tool's input properties.
    pub flags: Vec<ToolFlag>,
    /// An `mcp call` command line with every required flag.
    pub example: String,
}

impl RenderText for ToolPage {
    fn render_text(&self) -> String {
        let mut text = String::new();
        for item in &self.page.items {
            text.push_str(&format!("{} {}  {}\n", item.server, item.name, item.summary));
        }
        if self.page.has_more {
            text.push_str("More tools follow; pass --cursor with nextCursor.\n");
        }
        text.push_str(&unavailable_text(&self.unavailable));
        text
    }
}

impl RenderText for ToolMatches {
    fn render_text(&self) -> String {
        let mut text = String::new();
        for item in &self.items {
            text.push_str(&format!("{}  {}\n", item.command, item.summary));
        }
        if self.items.is_empty() {
            text.push_str("No tool matches. Try other words, or `mcp tools list`.\n");
        }
        text.push_str(&unavailable_text(&self.unavailable));
        text
    }
}

impl RenderText for ToolInfo {
    fn render_text(&self) -> String {
        crate::commands::call::usage(&self.server, &self.tool, &self.flags)
    }
}

fn unavailable_text(unavailable: &[Unavailable]) -> String {
    unavailable
        .iter()
        .map(|server| {
            format!(
                "unavailable: {} ({}): {}\n",
                server.server,
                server.code.as_str(),
                server.message
            )
        })
        .collect()
}

/// Each server's tools, and the servers that could not answer.
struct Gathered<'a> {
    answered: Vec<(&'a Server, Vec<Value>)>,
    unavailable: Vec<Unavailable>,
}

/// Every selected server's tools, fetched concurrently, in server-name order.
fn gather<'a>(connector: &'a Connector, invocation: &Invocation) -> Result<Gathered<'a>, Failure> {
    let wanted = invocation.strings("server");
    let servers: Vec<&Server> = if wanted.is_empty() {
        connector.configuration.servers()?.iter().collect()
    } else {
        wanted.iter().map(|name| connector.configuration.server(name)).collect::<Result<_, _>>()?
    };
    let answers: Vec<Result<Vec<Value>, Failure>> = std::thread::scope(|scope| {
        let handles: Vec<_> = servers
            .iter()
            .map(|server| scope.spawn(move || connector.tools(server).map(|(_, tools)| tools)))
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| Err(Failure::internal("a server thread panicked")))
            })
            .collect()
    });
    let mut answered = Vec::new();
    let mut unavailable = Vec::new();
    for (server, answer) in servers.into_iter().zip(answers) {
        match answer {
            Ok(tools) => answered.push((server, tools)),
            Err(failure) => unavailable.push(Unavailable {
                server: server.name.clone(),
                code: failure.code,
                message: failure.message,
            }),
        }
    }
    Ok(Gathered { answered, unavailable })
}

fn list(invocation: &Invocation) -> Result<Response, Failure> {
    let connector = Connector::load(invocation)?;
    let Gathered { answered, unavailable } = gather(&connector, invocation)?;
    let summaries: Vec<ToolSummary> = answered
        .iter()
        .flat_map(|(server, tools)| tools.iter().map(|tool| summarize(&server.name, tool)))
        .collect();
    let limit = invocation.integer("limit").unwrap_or(50);
    let page = search::page(summaries, invocation.string("cursor").as_deref(), limit)?;
    Ok(Response::new(&ToolPage { page, complete: unavailable.is_empty(), unavailable }))
}

fn summarize(server: &str, tool: &Value) -> ToolSummary {
    let description = tool["description"].as_str().unwrap_or_default();
    let line = description.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim();
    let truncated = line.chars().count() > SUMMARY_LENGTH || description.trim() != line;
    ToolSummary {
        server: server.to_string(),
        name: tool["name"].as_str().unwrap_or_default().to_string(),
        title: title(tool),
        summary: line.chars().take(SUMMARY_LENGTH).collect(),
        summary_truncated: truncated,
        annotations: tool.get("annotations").cloned(),
    }
}

fn title(tool: &Value) -> Option<String> {
    tool.get("title")
        .or_else(|| tool.get("annotations").and_then(|annotations| annotations.get("title")))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn search(invocation: &Invocation) -> Result<Response, Failure> {
    let connector = Connector::load(invocation)?;
    let terms = search::query_words(&invocation.strings("query").join(" "));
    let Gathered { answered, unavailable } = gather(&connector, invocation)?;
    let mut matches: Vec<ToolMatch> = answered
        .iter()
        .flat_map(|(server, tools)| tools.iter().map(move |tool| (server, tool)))
        .map(|(server, tool)| {
            let summary = summarize(&server.name, tool);
            ToolMatch {
                command: format!(
                    "{NAME} tools info {} {}",
                    shell_word(&server.name),
                    shell_word(&summary.name)
                ),
                score: search::rounded(score(&server.name, tool, &terms)),
                server: server.name.clone(),
                tool: summary.name,
                title: summary.title,
                summary: summary.summary,
            }
        })
        .filter(|item| item.score > 0.0)
        .collect();
    matches.sort_by(|left, right| right.score.total_cmp(&left.score));
    let limit = usize::try_from(invocation.integer("limit").unwrap_or(10)).unwrap_or(usize::MAX);
    let has_more = matches.len() > limit;
    matches.truncate(limit);
    Ok(Response::new(&ToolMatches {
        items: matches,
        has_more,
        complete: unavailable.is_empty(),
        unavailable,
    }))
}

/// The share of query words the tool matches, each weighted by where it matched.
fn score(server: &str, tool: &Value, terms: &[String]) -> f64 {
    if terms.is_empty() {
        return 0.0;
    }
    let words = |text: &str| search::words(&kebab_case(text));
    let name = words(tool["name"].as_str().unwrap_or_default());
    let title = words(&title(tool).unwrap_or_default());
    let description = search::words(tool["description"].as_str().unwrap_or_default());
    let schema = tool.get("inputSchema").cloned().unwrap_or(json!({}));
    let parameters: Vec<String> = flags(&schema)
        .iter()
        .flat_map(|flag| {
            words(&format!("{} {}", flag.property, flag.description.clone().unwrap_or_default()))
        })
        .collect();
    let server = words(server);
    let fields: [(&[String], f64); 5] =
        [(&name, 1.0), (&title, 0.8), (&description, 0.6), (&server, 0.5), (&parameters, 0.4)];
    let total: f64 = terms
        .iter()
        .map(|term| {
            fields
                .iter()
                .map(|(words, weight)| search::best_match(term, words) * weight)
                .fold(0.0, f64::max)
        })
        .sum();
    total / f64::from(u32::try_from(terms.len()).unwrap_or(u32::MAX))
}

fn info(invocation: &Invocation) -> Result<Response, Failure> {
    let connector = Connector::load(invocation)?;
    let server =
        connector.configuration.server(&invocation.string("server").unwrap_or_default())?;
    let tool_name = invocation.string("tool").unwrap_or_default();
    let (session, tool) = connector.tool(server, &tool_name)?;
    let flags = flags(tool.get("inputSchema").unwrap_or(&json!({})));
    let example = crate::commands::call::example(&server.name, &tool_name, &flags);
    Ok(Response::new(&ToolInfo { server: server.name.clone(), session, tool, flags, example }))
}
