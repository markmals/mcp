//! `mcp docs`: searching and reading the documentation compiled into the binary.

use schemars::{JsonSchema, schema_for};
use serde::Serialize;
use serde_json::json;

use crate::NAME;
use crate::commands::cli::{SEARCH_CURSOR, SEARCH_LIMIT};
use crate::docs::{self, DOCUMENTS, Document};
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::registry::{ArgumentKind, ArgumentSpec, CommandSpec, Effect, Example, ValueKind};
use crate::response::{RenderText, Response};
use crate::search::{self, Page};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "docs.search",
        path: &["docs", "search"],
        summary: "Search mcp's documentation: configuration, approval, sessions, tools, the CLI",
        description: "Ranks every section of the Markdown compiled into this binary against the \
            query and returns the best matches: the document `path`, its `title`, the `section` \
            heading, an `excerpt` of the best-matching paragraph, and a `score`. Read a whole \
            document with `mcp docs show <path>`. Runs offline and never changes anything.",
        arguments: &[
            ArgumentSpec {
                name: "query",
                kind: ArgumentKind::Positional,
                value: ValueKind::String,
                required: true,
                default: None,
                multiple: true,
                summary: "The topic, in plain words; several words need no quotes",
            },
            SEARCH_LIMIT,
            SEARCH_CURSOR,
        ],
        effect: Effect::Reads,
        requires: &[],
        dry_run: false,
        idempotent: true,
        errors: &[ErrorCode::InvalidArgument],
        examples: &[Example {
            summary: "Learn how list pages continue",
            args: &["docs", "search", "pagination cursor"],
        }],
        synonyms: &["documentation", "guide", "manual", "learn", "concept", "explain", "help"],
        output_schema: || schema_for!(Page<SectionMatch>),
        run: search_documents,
    },
    CommandSpec {
        id: "docs.show",
        path: &["docs", "show"],
        summary: "Print one documentation file in full",
        description: "Returns the Markdown of one document compiled into this binary, by the \
            `path` that `mcp docs search` reports, such as `docs/tools.md`. `--output text` \
            prints the Markdown alone. Never changes anything.",
        arguments: &[ArgumentSpec {
            name: "path",
            kind: ArgumentKind::Positional,
            value: ValueKind::String,
            required: true,
            default: None,
            multiple: false,
            summary: "The document, as `docs search` names it: `docs/tools.md`",
        }],
        effect: Effect::Reads,
        requires: &[],
        dry_run: false,
        idempotent: true,
        errors: &[ErrorCode::NotFound],
        examples: &[Example {
            summary: "Read how tool flags work",
            args: &["docs", "show", "docs/tools.md"],
        }],
        synonyms: &["read", "open", "view", "print", "document", "file"],
        output_schema: || schema_for!(DocumentContent),
        run: show_document,
    },
];

/// One section that matches the query.
#[derive(Serialize, JsonSchema)]
pub struct SectionMatch {
    /// The document, for `mcp docs show <path>`.
    pub path: String,
    /// The document's title.
    pub title: String,
    /// The heading of the matching section.
    pub section: String,
    /// The paragraph of the section that best matches the query, shortened to 400 characters.
    pub excerpt: String,
    /// Relevance from 0 to 1.
    pub score: f64,
}

impl RenderText for Page<SectionMatch> {
    fn render_text(&self) -> String {
        if self.items.is_empty() {
            return "No documentation matches.".into();
        }
        let mut text = String::new();
        for item in &self.items {
            text.push_str(&format!(
                "{} — {} › {}\n    {}\n",
                item.path, item.title, item.section, item.excerpt
            ));
        }
        if self.has_more {
            text.push_str("More matches follow; pass --cursor with nextCursor or raise --limit.\n");
        }
        text
    }
}

/// A whole document.
#[derive(Serialize, JsonSchema)]
pub struct DocumentContent {
    pub path: String,
    pub title: String,
    /// The document's Markdown.
    pub markdown: String,
}

impl RenderText for DocumentContent {
    fn render_text(&self) -> String {
        self.markdown.clone()
    }
}

fn search_documents(invocation: &Invocation) -> Result<Response, Failure> {
    let terms = search::query_words(&invocation.strings("query").join(" "));
    let mut ranked: Vec<SectionMatch> =
        DOCUMENTS.iter().flat_map(|document| score_sections(document, &terms)).collect();
    ranked.sort_by(|left, right| right.score.total_cmp(&left.score));
    let limit = invocation.integer("limit").unwrap_or(5);
    let page = search::page(ranked, invocation.string("cursor").as_deref(), limit)?;
    Ok(Response::new(&page))
}

fn score_sections(document: &Document, terms: &[String]) -> Vec<SectionMatch> {
    if terms.is_empty() {
        return Vec::new();
    }
    let title = document.title();
    let title_words = search::words(title);
    document
        .sections()
        .into_iter()
        .filter_map(|section| {
            let heading_words = search::words(section.heading);
            let body_words = search::words(&section.body);
            let total: f64 = terms
                .iter()
                .map(|term| {
                    let occurrences = body_words
                        .iter()
                        .filter(|word| search::word_match(term, word) > 0.0)
                        .count()
                        .min(5);
                    let body = if occurrences == 0 {
                        0.0
                    } else {
                        0.3 + 0.03 * f64::from(u8::try_from(occurrences).unwrap_or(5))
                    };
                    [
                        search::best_match(term, &heading_words),
                        search::best_match(term, &title_words) * 0.5,
                        body,
                    ]
                    .into_iter()
                    .fold(0.0, f64::max)
                })
                .sum();
            let score = total / f64::from(u32::try_from(terms.len()).unwrap_or(u32::MAX));
            (score > 0.0).then(|| SectionMatch {
                path: document.path.to_string(),
                title: title.to_string(),
                section: if section.heading.is_empty() { title } else { section.heading }
                    .to_string(),
                excerpt: excerpt(&section.body, terms),
                score: search::rounded(score),
            })
        })
        .collect()
}

/// The paragraph with the most query words, outside code blocks, shortened to 400 characters.
fn excerpt(body: &str, terms: &[String]) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut fenced = false;
    for line in body.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if line.trim().is_empty() {
            paragraphs.push(std::mem::take(&mut current));
        } else {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(line.trim());
        }
    }
    paragraphs.push(current);
    let best = paragraphs
        .iter()
        .filter(|paragraph| !paragraph.is_empty())
        .map(|paragraph| {
            let words = search::words(paragraph);
            let hits = terms.iter().filter(|term| search::best_match(term, &words) > 0.0).count();
            (hits, paragraph)
        })
        .fold(
            (0, None),
            |best, (hits, paragraph)| {
                if hits > best.0 { (hits, Some(paragraph)) } else { best }
            },
        )
        .1
        .or_else(|| paragraphs.iter().find(|paragraph| !paragraph.is_empty()));
    best.map(|paragraph| truncate(paragraph, 400)).unwrap_or_default()
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let cut: String = text.chars().take(limit).collect();
    format!("{}…", cut.trim_end())
}

fn show_document(invocation: &Invocation) -> Result<Response, Failure> {
    let path = invocation.string("path").unwrap_or_default();
    let document = docs::find(&path).ok_or_else(|| {
        Failure::not_found(format!("no document `{path}`"))
            .with_details(json!({
                "path": path,
                "documents": DOCUMENTS.iter().map(|document| document.path).collect::<Vec<_>>(),
            }))
            .suggest(format!("{NAME} docs search \"<topic>\""))
    })?;
    Ok(Response::new(&DocumentContent {
        path: document.path.to_string(),
        title: document.title().to_string(),
        markdown: document.source.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_excerpt_is_the_paragraph_with_the_most_query_words() {
        let body = "Nothing here.\n\nA cursor resumes a page.\nIt continues pagination.\n\n```sh\ncursor pagination cursor\n```\n";
        let terms = search::query_words("pagination cursor");
        assert_eq!(excerpt(body, &terms), "A cursor resumes a page. It continues pagination.");
    }

    #[test]
    fn long_excerpts_are_shortened_with_an_ellipsis() {
        assert_eq!(truncate(&"x".repeat(500), 400).chars().count(), 401);
    }
}
