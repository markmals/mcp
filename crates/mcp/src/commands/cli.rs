//! `mcp cli search`: finding the command for a task.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;

use crate::NAME;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{self, ArgumentKind, ArgumentSpec, CommandSpec, Effect, Example, ValueKind};
use crate::response::{RenderText, Response};
use crate::search::{self, Page};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "cli.search",
    path: &["cli", "search"],
    summary: "Find the mcp command for a task described in plain words",
    description: "Ranks every command against the query by its name, summary, description, \
        arguments, and declared synonyms, and returns the best matches with the exact command to \
        run and whether it reads or modifies state. Runs locally and never changes anything. \
        Inspect a match with `mcp schema <command>` before running it.",
    arguments: &[
        ArgumentSpec {
            name: "query",
            kind: ArgumentKind::Positional,
            value: ValueKind::String,
            required: true,
            default: None,
            multiple: true,
            summary: "What you want to do, in plain words; several words need no quotes",
        },
        SEARCH_LIMIT,
        SEARCH_CURSOR,
    ],
    effect: Effect::Reads,
    requires: &[],
    dry_run: false,
    idempotent: true,
    errors: &[ErrorCode::InvalidArgument],
    examples: &[
        Example { summary: "Find how to call a tool", args: &["cli", "search", "call a tool"] },
        Example {
            summary: "Find how to approve a project's servers, three matches at most",
            args: &["cli", "search", "trust the servers in this repository", "--limit", "3"],
        },
    ],
    synonyms: &["find", "discover", "lookup", "help", "commands", "which", "how"],
    output_schema: || schema_for!(Page<CommandMatch>),
    run: search_commands,
}];

/// `--limit` for the searches: few, ranked matches.
pub const SEARCH_LIMIT: ArgumentSpec = ArgumentSpec {
    name: "limit",
    kind: ArgumentKind::Option,
    value: ValueKind::Integer { min: 1, max: Some(20) },
    required: false,
    default: Some("5"),
    multiple: false,
    summary: "The most matches to return",
};

/// `--cursor` for the searches.
pub const SEARCH_CURSOR: ArgumentSpec = ArgumentSpec {
    name: "cursor",
    kind: ArgumentKind::Option,
    value: ValueKind::String,
    required: false,
    default: None,
    multiple: false,
    summary: "Resume after a previous page: the nextCursor it returned",
};

/// One command that fits the query.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandMatch {
    /// The exact command to run, without its arguments.
    pub command: String,
    /// The registry identifier, such as `tools.info`.
    pub id: String,
    pub summary: String,
    /// `reads`, or `modifies <what>`.
    pub effect: String,
    /// Relevance from 0 to 1.
    pub score: f64,
}

impl RenderText for Page<CommandMatch> {
    fn render_text(&self) -> String {
        if self.items.is_empty() {
            return format!(
                "No command matches. Try other words, or `{NAME} schema` for the list."
            );
        }
        let width = self.items.iter().map(|item| item.command.len()).max().unwrap_or(0);
        let mut text = String::new();
        for item in &self.items {
            text.push_str(&format!("{:width$}  {}\n", item.command, item.summary));
        }
        if self.has_more {
            text.push_str("More matches follow; pass --cursor with nextCursor or raise --limit.\n");
        }
        text
    }
}

fn search_commands(invocation: &Invocation) -> Result<Response, Failure> {
    let query = invocation.strings("query").join(" ");
    let limit = invocation.integer("limit").unwrap_or(5);
    let page = search::page(rank(&query), invocation.string("cursor").as_deref(), limit)?;
    Ok(Response::new(&page))
}

/// Every command with a positive score for `query`, best first, ties in registry order.
pub fn rank(query: &str) -> Vec<CommandMatch> {
    let terms = search::query_words(query);
    let mut scored: Vec<(f64, &CommandSpec)> = registry::all()
        .into_iter()
        .map(|command| (score(command, &terms), command))
        .filter(|(score, _)| *score > 0.0)
        .collect();
    scored.sort_by(|left, right| right.0.total_cmp(&left.0));
    scored
        .into_iter()
        .map(|(score, command)| CommandMatch {
            command: command.command_line(),
            id: command.id.to_string(),
            summary: command.summary.to_string(),
            effect: command.effect_label(),
            score: search::rounded(score),
        })
        .collect()
}

/// The share of query words the command matches, each weighted by where it matched.
fn score(command: &CommandSpec, terms: &[String]) -> f64 {
    if terms.is_empty() {
        return 0.0;
    }
    let path: Vec<String> = command.path.iter().flat_map(|word| search::words(word)).collect();
    let synonyms: Vec<String> =
        command.synonyms.iter().flat_map(|word| search::words(word)).collect();
    let summary = search::words(command.summary);
    let arguments: Vec<String> = command
        .arguments
        .iter()
        .flat_map(|argument| search::words(&format!("{} {}", argument.name, argument.summary)))
        .collect();
    let description = search::words(command.description);

    let fields: [(&[String], f64); 5] =
        [(&path, 1.0), (&synonyms, 0.8), (&summary, 0.7), (&arguments, 0.4), (&description, 0.3)];
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

#[cfg(test)]
mod tests {
    use super::*;

    fn top(query: &str) -> String {
        rank(query).first().map(|item| item.id.clone()).unwrap_or_default()
    }

    #[test]
    fn ranks_the_intended_command_first() {
        assert_eq!(top("call a tool"), "call");
        assert_eq!(top("run a tool with arguments"), "call");
        assert_eq!(top("find a tool for taking a screenshot"), "tools.search");
        assert_eq!(top("list configured servers"), "servers.list");
        assert_eq!(top("trust the servers in this repository"), "servers.approve");
        assert_eq!(top("which config file is used"), "context.show");
        assert_eq!(top("restart a server"), "servers.disconnect");
        assert_eq!(top("what arguments does a tool take"), "tools.info");
        assert_eq!(top("find a command"), "cli.search");
        assert_eq!(top("search the documentation"), "docs.search");
        assert_eq!(top("read a documentation file"), "docs.show");
    }

    #[test]
    fn returns_nothing_for_unrelated_words() {
        assert!(rank("zebra xylophone").is_empty());
    }
}
