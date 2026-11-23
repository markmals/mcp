//! The command registry: the one declaration of every operation `mcp` performs.
//!
//! The parser, `--help`, `mcp schema`, the `mcp cli search` index, and the CLI reference
//! (`docs/cli.md`) are all generated from these entries, and the tests in `tests/cli/registry.rs`
//! fail when the parser, the runtime defaults, or the examples drift from them.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::NAME;
use crate::failure::{ErrorCode, Failure};
use crate::invocation::Invocation;
use crate::response::Response;

/// One operation, declared once.
pub struct CommandSpec {
    /// Stable identifier, the command path joined with dots: `notes.create`.
    pub id: &'static str,
    /// The words after `mcp`: `["notes", "create"]`.
    pub path: &'static [&'static str],
    /// One line, shown in search results and `--help` listings.
    pub summary: &'static str,
    /// Everything an agent needs to use the command correctly, in prose.
    pub description: &'static str,
    pub arguments: &'static [ArgumentSpec],
    pub effect: Effect,
    pub requires: &'static [Requirement],
    /// Whether the command accepts `--dry-run`. Every modifying command does.
    pub dry_run: bool,
    /// Whether running it twice with the same input leaves the same state as running it once.
    pub idempotent: bool,
    /// Every error code the command can return, besides `INTERNAL`.
    pub errors: &'static [ErrorCode],
    /// At least one example. Each one runs against a seeded store in the test suite, and its
    /// result must match `output_schema`.
    pub examples: &'static [Example],
    /// Words an agent might use for this operation that its name and summary do not contain.
    pub synonyms: &'static [&'static str],
    /// JSON Schema for the result on stdout.
    pub output_schema: fn() -> schemars::Schema,
    pub run: fn(&Invocation) -> Result<Response, Failure>,
}

/// Whether an operation reads or modifies state, and what it modifies.
#[derive(Clone, Copy)]
pub enum Effect {
    Reads,
    Modifies(&'static str),
    /// Starts or reuses server sessions, and asks servers what they offer; never calls a tool.
    Connects,
    /// Calls a server's tool, whose effects belong to the server.
    Calls,
}

/// What an operation needs at run time beyond `mcp` itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// The user's and the project's `.mcp.json`, which `mcp context show` reports.
    Configuration,
    /// The configured servers themselves, reached through the session daemon.
    Servers,
    /// A person at the machine, who confirms with the system's authentication.
    Person,
}

impl Requirement {
    pub fn as_str(self) -> &'static str {
        match self {
            Requirement::Configuration => "configuration",
            Requirement::Servers => "servers",
            Requirement::Person => "person",
        }
    }
}

pub struct ArgumentSpec {
    /// Kebab-case. A flag or option is passed as `--<name>`; a positional is passed bare.
    pub name: &'static str,
    pub kind: ArgumentKind,
    pub value: ValueKind,
    pub required: bool,
    /// The value used when the argument is absent. Clap applies it, so it is the runtime default.
    pub default: Option<&'static str>,
    /// A positional or option that accepts several values.
    pub multiple: bool,
    pub summary: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArgumentKind {
    Positional,
    Option,
    Flag,
    /// Every word after the positionals, flags included, passed through unparsed.
    Trailing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    String,
    Path,
    /// A whole number the parser checks against its bounds.
    Integer {
        min: u64,
        max: Option<u64>,
    },
    /// Free text, `@<path>` to read a file, or `-` to read stdin.
    Text,
    Choice(&'static [&'static str]),
    Boolean,
}

impl ValueKind {
    pub fn type_name(self) -> &'static str {
        match self {
            ValueKind::String => "string",
            ValueKind::Path => "path",
            ValueKind::Integer { .. } => "integer",
            ValueKind::Text => "text",
            ValueKind::Choice(_) => "choice",
            ValueKind::Boolean => "boolean",
        }
    }

    pub fn accepted_values(self) -> Option<&'static [&'static str]> {
        match self {
            ValueKind::Choice(values) => Some(values),
            _ => None,
        }
    }
}

pub struct Example {
    pub summary: &'static str,
    /// The arguments after `mcp`.
    pub args: &'static [&'static str],
}

/// Options every command accepts.
pub const GLOBAL_ARGUMENTS: &[ArgumentSpec] = &[ArgumentSpec {
    name: "output",
    kind: ArgumentKind::Option,
    value: ValueKind::Choice(&["json", "text"]),
    required: false,
    default: Some("json"),
    multiple: false,
    summary: "Result format: JSON (compact when piped, indented on a terminal) or text for humans",
}];

/// The `--dry-run` flag every modifying command declares.
pub const DRY_RUN: ArgumentSpec = ArgumentSpec {
    name: "dry-run",
    kind: ArgumentKind::Flag,
    value: ValueKind::Boolean,
    required: false,
    default: None,
    multiple: false,
    summary: "Validate the input and resolve the target, and return what the command would do, \
              without changing anything",
};

/// `--timeout` on every command that contacts a server.
pub const TIMEOUT: ArgumentSpec = ArgumentSpec {
    name: "timeout",
    kind: ArgumentKind::Option,
    value: ValueKind::Integer { min: 1, max: Some(3600) },
    required: false,
    default: Some("120"),
    multiple: false,
    summary: "Seconds to wait for each server, starting its session and answering together",
};

/// `--server` on the commands that ask several servers.
pub const SERVER_FILTER: ArgumentSpec = ArgumentSpec {
    name: "server",
    kind: ArgumentKind::Option,
    value: ValueKind::String,
    required: false,
    default: None,
    multiple: true,
    summary: "Ask only this server; repeat for several. Default: every configured server",
};

/// `--all` on the commands that act on several named servers.
pub const ALL_SERVERS: ArgumentSpec = ArgumentSpec {
    name: "all",
    kind: ArgumentKind::Flag,
    value: ValueKind::Boolean,
    required: false,
    default: None,
    multiple: false,
    summary: "Every server instead of the named ones",
};

/// The resource groups, in the order `--help` lists them, with a summary of each.
pub const RESOURCES: &[(&str, &str)] = &[
    ("cli", "Find the command for a task"),
    ("schema", "Inspect a command's full specification"),
    ("docs", "Search and read mcp's documentation"),
    ("context", "Show the configuration, workspace, approvals, and daemon in effect"),
    ("servers", "List, inspect, approve, and disconnect configured MCP servers"),
    ("tools", "List, search, and inspect the tools servers offer"),
    ("call", "Call a server's tool"),
];

/// Every command, in the order discovery lists them.
pub fn all() -> Vec<&'static CommandSpec> {
    crate::commands::GROUPS.iter().flat_map(|group| group.iter()).collect()
}

pub fn find(path: &[&str]) -> Option<&'static CommandSpec> {
    all().into_iter().find(|command| command.path == path)
}

impl CommandSpec {
    pub fn command_line(&self) -> String {
        format!("{NAME} {}", self.path.join(" "))
    }

    pub fn effect_label(&self) -> String {
        match self.effect {
            Effect::Reads => "reads".to_string(),
            Effect::Modifies(what) => format!("modifies {what}"),
            Effect::Connects => "connects to servers".to_string(),
            Effect::Calls => "calls a tool; its effects belong to the server".to_string(),
        }
    }

    /// The command's arguments followed by the global ones.
    pub fn all_arguments(&self) -> impl Iterator<Item = &'static ArgumentSpec> {
        self.arguments.iter().chain(GLOBAL_ARGUMENTS.iter())
    }

    /// The full entry `mcp schema <command>` returns.
    pub fn describe(&self) -> CommandDescription {
        let errors = self
            .errors
            .iter()
            .chain(std::iter::once(&ErrorCode::Internal))
            .map(|code| ErrorDescription {
                code: code.as_str().to_string(),
                exit: code.exit_status(),
                retryable: code.retryable(),
                meaning: code.meaning().to_string(),
            })
            .collect();
        CommandDescription {
            id: self.id.to_string(),
            command: self.command_line(),
            summary: self.summary.to_string(),
            description: self.description.to_string(),
            arguments: self.all_arguments().map(ArgumentSpec::describe).collect(),
            effect: match self.effect {
                Effect::Reads => EffectDescription { kind: "reads".into(), what: None },
                Effect::Modifies(what) => {
                    EffectDescription { kind: "modifies".into(), what: Some(what.to_string()) }
                }
                Effect::Connects => EffectDescription {
                    kind: "connects".into(),
                    what: Some("starts or reuses server sessions; never calls a tool".into()),
                },
                Effect::Calls => EffectDescription {
                    kind: "calls".into(),
                    what: Some("a server's tool, whose effects belong to the server".into()),
                },
            },
            requires: self
                .requires
                .iter()
                .map(|requirement| requirement.as_str().to_string())
                .collect(),
            dry_run: self.dry_run,
            idempotent: self.idempotent,
            errors,
            examples: self
                .examples
                .iter()
                .map(|example| ExampleDescription {
                    summary: example.summary.to_string(),
                    command: example_command_line(example),
                })
                .collect(),
            output: (self.output_schema)().to_value(),
        }
    }
}

/// A command's full specification, as `mcp schema <command>` returns it.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandDescription {
    /// Stable identifier: the command path joined with dots.
    pub id: String,
    /// The command to run, without arguments.
    pub command: String,
    pub summary: String,
    pub description: String,
    /// The command's own arguments, then the options every command accepts.
    pub arguments: Vec<ArgumentDescription>,
    pub effect: EffectDescription,
    /// What the command needs besides mcp: `configuration` (the `.mcp.json` files `mcp
    /// context show` reports), `servers` (the configured servers, through the session daemon),
    /// or `person` (someone at the machine to confirm).
    pub requires: Vec<String>,
    /// Whether the command accepts `--dry-run`.
    pub dry_run: bool,
    /// Whether running it twice with the same input leaves the same state as running it once.
    pub idempotent: bool,
    /// Every error the command can report on stderr.
    pub errors: Vec<ErrorDescription>,
    pub examples: Vec<ExampleDescription>,
    /// JSON Schema of the result the command writes to stdout.
    pub output: Value,
}

#[derive(Serialize, JsonSchema)]
pub struct EffectDescription {
    /// `reads`, `modifies`, `connects` (starts or reuses server sessions), or `calls` (a
    /// server's tool).
    pub kind: String,
    /// What the command changes, connects to, or calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub what: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct ErrorDescription {
    pub code: String,
    /// The exit status the error sets.
    pub exit: u8,
    /// Whether retrying the same command can succeed.
    pub retryable: bool,
    pub meaning: String,
}

#[derive(Serialize, JsonSchema)]
pub struct ExampleDescription {
    pub summary: String,
    pub command: String,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArgumentDescription {
    pub name: String,
    /// `positional`, `option`, `flag`, or `trailing` (every word after the positionals, passed
    /// through to the tool).
    pub kind: String,
    /// How to write it: `<name>`, `--limit <integer>`, `--dry-run`, `[--<property> <value>]...`.
    pub usage: String,
    /// `string`, `path`, `integer`, `text` (text, `@<file>`, or `-` for stdin), `choice`, or
    /// `boolean`.
    #[serde(rename = "type")]
    pub value_type: String,
    pub required: bool,
    pub default: Option<String>,
    /// Whether it accepts several values.
    pub multiple: bool,
    /// The only values a `choice` accepts.
    pub accepted_values: Option<Vec<String>>,
    /// The smallest `integer` accepted.
    pub minimum: Option<u64>,
    /// The largest `integer` accepted.
    pub maximum: Option<u64>,
    pub summary: String,
}

pub fn example_command_line(example: &Example) -> String {
    let words: Vec<String> = example.args.iter().map(|arg| shell_word(arg)).collect();
    format!("{NAME} {}", words.join(" "))
}

/// `word` as a POSIX shell reads it back: unquoted when it is plain, else single-quoted.
pub fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_./@=:".contains(character));
    if plain { word.to_string() } else { format!("'{}'", word.replace('\'', r"'\''")) }
}

impl ArgumentSpec {
    pub fn usage(&self) -> String {
        match self.kind {
            ArgumentKind::Positional => format!("<{}>", self.name),
            ArgumentKind::Option => format!("--{} <{}>", self.name, self.value.type_name()),
            ArgumentKind::Flag => format!("--{}", self.name),
            ArgumentKind::Trailing => "[--<property> <value>]...".to_string(),
        }
    }

    pub fn describe(&self) -> ArgumentDescription {
        let (minimum, maximum) = match self.value {
            ValueKind::Integer { min, max } => (Some(min), max),
            _ => (None, None),
        };
        ArgumentDescription {
            name: self.name.to_string(),
            kind: match self.kind {
                ArgumentKind::Positional => "positional",
                ArgumentKind::Option => "option",
                ArgumentKind::Flag => "flag",
                ArgumentKind::Trailing => "trailing",
            }
            .to_string(),
            usage: self.usage(),
            value_type: self.value.type_name().to_string(),
            required: self.required,
            default: self.default.map(str::to_string),
            multiple: self.multiple,
            accepted_values: self
                .value
                .accepted_values()
                .map(|values| values.iter().map(|value| (*value).to_string()).collect()),
            minimum,
            maximum,
            summary: self.summary.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_quote_words_a_shell_would_split() {
        let example =
            Example { summary: "", args: &["call", "demo", "echo", "--text", "Bob's list"] };
        assert_eq!(
            example_command_line(&example),
            format!("{NAME} call demo echo --text 'Bob'\\''s list'")
        );
    }
}
