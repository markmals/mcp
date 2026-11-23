//! One server entry of a `.mcp.json` file: what it says, as written, and whether `mcp` can use it.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};

/// How `mcp` talks to a server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TransportKind {
    Stdio,
    Http,
}

/// A usable entry, as written: values still hold their `${VAR}` references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    Stdio { command: String, args: Vec<String>, env: BTreeMap<String, String> },
    Http { url: String, headers: BTreeMap<String, String> },
}

/// Why an entry is unusable: the field at fault, when one is, and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryProblem {
    pub field: Option<String>,
    pub message: String,
}

impl EntryProblem {
    fn field(field: &str, message: impl Into<String>) -> EntryProblem {
        EntryProblem { field: Some(field.to_string()), message: message.into() }
    }

    fn entry(message: impl Into<String>) -> EntryProblem {
        EntryProblem { field: None, message: message.into() }
    }
}

const STDIO_FIELDS: &[&str] = &["command", "args", "env"];
const HTTP_FIELDS: &[&str] = &["url", "headers"];

impl Entry {
    pub fn transport(&self) -> TransportKind {
        match self {
            Entry::Stdio { .. } => TransportKind::Stdio,
            Entry::Http { .. } => TransportKind::Http,
        }
    }

    /// Validates a server entry as written.
    ///
    /// # Errors
    ///
    /// The first problem that makes the entry unusable.
    pub fn parse(name: &str, written: &Value) -> Result<Entry, EntryProblem> {
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(EntryProblem::entry(
                "a server name must be nonempty and hold no control characters",
            ));
        }
        let Value::Object(fields) = written else {
            return Err(EntryProblem::entry("a server entry must be a JSON object"));
        };
        if let Some(unknown) = fields.keys().find(|key| {
            key.as_str() != "type"
                && !STDIO_FIELDS.contains(&key.as_str())
                && !HTTP_FIELDS.contains(&key.as_str())
        }) {
            return Err(EntryProblem::field(
                unknown,
                format!("`{unknown}` is not a field mcp supports; it never ignores a field"),
            ));
        }
        match transport(fields)? {
            TransportKind::Stdio => parse_stdio(fields),
            TransportKind::Http => parse_http(fields),
        }
    }
}

/// The transport `type` names, or the one `command` or `url` implies.
fn transport(fields: &Map<String, Value>) -> Result<TransportKind, EntryProblem> {
    let declared = match fields.get("type") {
        None => None,
        Some(Value::String(kind)) if kind == "stdio" => Some(TransportKind::Stdio),
        Some(Value::String(kind)) if kind == "http" => Some(TransportKind::Http),
        Some(Value::String(kind)) if kind == "sse" => {
            return Err(EntryProblem::field(
                "type",
                "`type: \"sse\"` is the legacy HTTP+SSE transport, which mcp does not support; \
                 use the server's Streamable HTTP endpoint with `type: \"http\"`",
            ));
        }
        Some(_) => {
            return Err(EntryProblem::field("type", "`type` must be \"stdio\" or \"http\""));
        }
    };
    let implied = match (fields.contains_key("command"), fields.contains_key("url")) {
        (true, false) => TransportKind::Stdio,
        (false, true) => TransportKind::Http,
        (true, true) => {
            return Err(EntryProblem::entry("a server has either `command` or `url`, not both"));
        }
        (false, false) => {
            return Err(EntryProblem::entry("a server needs `command` (stdio) or `url` (http)"));
        }
    };
    match declared {
        Some(kind) if kind != implied => {
            let field = if implied == TransportKind::Stdio { "command" } else { "url" };
            Err(EntryProblem::field(field, wrong_transport(field, kind)))
        }
        _ => Ok(implied),
    }
}

fn wrong_transport(field: &str, kind: TransportKind) -> String {
    let kind = match kind {
        TransportKind::Stdio => "stdio",
        TransportKind::Http => "http",
    };
    format!("`{field}` does not belong to a {kind} server")
}

fn parse_stdio(fields: &Map<String, Value>) -> Result<Entry, EntryProblem> {
    if let Some(field) = HTTP_FIELDS.iter().find(|field| fields.contains_key(**field)) {
        return Err(EntryProblem::field(field, wrong_transport(field, TransportKind::Stdio)));
    }
    let command = match fields.get("command") {
        Some(Value::String(command)) if !command.is_empty() => command.clone(),
        _ => return Err(EntryProblem::field("command", "`command` must be a nonempty string")),
    };
    let args = match fields.get("args") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| EntryProblem::field("args", "`args` must be an array of strings"))?,
        Some(_) => return Err(EntryProblem::field("args", "`args` must be an array of strings")),
    };
    Ok(Entry::Stdio { command, args, env: string_map(fields, "env")? })
}

fn parse_http(fields: &Map<String, Value>) -> Result<Entry, EntryProblem> {
    if let Some(field) = STDIO_FIELDS.iter().find(|field| fields.contains_key(**field)) {
        return Err(EntryProblem::field(field, wrong_transport(field, TransportKind::Http)));
    }
    let url = match fields.get("url") {
        Some(Value::String(url)) if !url.is_empty() => url.clone(),
        _ => return Err(EntryProblem::field("url", "`url` must be a nonempty string")),
    };
    Ok(Entry::Http { url, headers: string_map(fields, "headers")? })
}

fn string_map(
    fields: &Map<String, Value>,
    field: &str,
) -> Result<BTreeMap<String, String>, EntryProblem> {
    let problem = || EntryProblem::field(field, format!("`{field}` must map names to strings"));
    match fields.get(field) {
        None => Ok(BTreeMap::new()),
        Some(Value::Object(entries)) => entries
            .iter()
            .map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
            .collect::<Option<BTreeMap<_, _>>>()
            .ok_or_else(problem),
        Some(_) => Err(problem()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn problem(entry: Value) -> EntryProblem {
        Entry::parse("server", &entry).unwrap_err()
    }

    #[test]
    fn reads_the_portable_fields_and_infers_the_transport() {
        let stdio = Entry::parse(
            "browser",
            &json!({"command": "npx", "args": ["-y", "x"], "env": {"A": "1"}}),
        )
        .unwrap();
        assert_eq!(stdio.transport(), TransportKind::Stdio);
        let http = Entry::parse(
            "figma",
            &json!({"type": "http", "url": "http://127.0.0.1/mcp", "headers": {"X": "y"}}),
        )
        .unwrap();
        assert_eq!(
            http,
            Entry::Http {
                url: "http://127.0.0.1/mcp".into(),
                headers: BTreeMap::from([("X".into(), "y".into())])
            }
        );
    }

    #[test]
    fn names_a_field_it_does_not_implement_instead_of_ignoring_it() {
        for field in ["cwd", "auth", "enabled", "timeout"] {
            let problem = problem(json!({"command": "x", field: "y"}));
            assert_eq!(problem.field.as_deref(), Some(field));
        }
        assert_eq!(
            problem(json!({"type": "sse", "url": "http://x"})).field.as_deref(),
            Some("type")
        );
    }

    #[test]
    fn rejects_fields_of_the_other_transport() {
        assert_eq!(
            problem(json!({"command": "x", "headers": {}})).field.as_deref(),
            Some("headers")
        );
        assert_eq!(problem(json!({"url": "http://x", "env": {}})).field.as_deref(), Some("env"));
        assert_eq!(
            problem(json!({"type": "http", "command": "x"})).field.as_deref(),
            Some("command")
        );
    }

    #[test]
    fn needs_exactly_one_of_command_and_url() {
        assert_eq!(problem(json!({})).field, None);
        assert_eq!(problem(json!({"command": "x", "url": "http://x"})).field, None);
    }

    #[test]
    fn checks_value_types() {
        assert_eq!(problem(json!({"command": ""})).field.as_deref(), Some("command"));
        assert_eq!(problem(json!({"command": "x", "args": [1]})).field.as_deref(), Some("args"));
        assert_eq!(problem(json!({"command": "x", "env": {"A": 1}})).field.as_deref(), Some("env"));
        assert_eq!(problem(json!({"type": 3, "command": "x"})).field.as_deref(), Some("type"));
        assert!(Entry::parse("bad\nname", &json!({"command": "x"})).is_err());
        assert!(Entry::parse("", &json!({"command": "x"})).is_err());
        assert!(Entry::parse("x", &json!("npx")).is_err());
    }
}
