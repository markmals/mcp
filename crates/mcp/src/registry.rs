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
}

/// What an operation needs at run time beyond `mcp` itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// The note store that `--store`, `MCP_STORE`, or the default location selects.
    Store,
}

impl Requirement {
    pub fn as_str(self) -> &'static str {
        match self {
            Requirement::Store => "store",
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
    /// A note name: see [`NAME_PATTERN`].
    Name,
    /// Free text, `@<path>` to read a file, or `-` to read stdin.
    Text,
    Choice(&'static [&'static str]),
    Boolean,
}

/// What a note name looks like: lowercase letters and digits in hyphen-separated words.
pub const NAME_PATTERN: &str = "^[a-z0-9]+(-[a-z0-9]+)*$";

/// The longest note name, in characters.
pub const NAME_MAX_LENGTH: usize = 64;

impl ValueKind {
    pub fn type_name(self) -> &'static str {
        match self {
            ValueKind::String => "string",
            ValueKind::Path => "path",
            ValueKind::Integer { .. } => "integer",
            ValueKind::Name => "name",
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
    summary: "Validate input, resolve the store, and return the receipt the command would produce, \
              without writing anything",
};

/// The `--store` option every command that touches the store declares.
pub const STORE: ArgumentSpec = ArgumentSpec {
    name: "store",
    kind: ArgumentKind::Option,
    value: ValueKind::Path,
    required: false,
    default: None,
    multiple: false,
    summary: "The note store, a SQLite file; overrides MCP_STORE and the default location. \
              `mcp context show` prints the store in effect",
};

/// The resource groups, in the order `--help` lists them, with a summary of each.
pub const RESOURCES: &[(&str, &str)] = &[
    ("cli", "Find the command for a task"),
    ("schema", "Inspect a command's full specification"),
    ("docs", "Search and read mcp's documentation"),
    ("context", "Show where commands take effect"),
    ("notes", "Create, read, update, and delete notes"),
    ("store", "Prepare and upgrade the note store"),
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
    /// What the command needs besides mcp: `store` is the note store `mcp context show`
    /// reports.
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
    /// `reads` or `modifies`.
    pub kind: String,
    /// What a modifying command changes.
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
    /// `positional`, `option`, or `flag`.
    pub kind: String,
    /// How to write it: `<name>`, `--limit <integer>`, `--dry-run`.
    pub usage: String,
    /// `string`, `path`, `integer`, `name` (matches `pattern`), `text` (text, `@<file>`, or `-`
    /// for stdin), `choice`, or `boolean`.
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
    /// The regular expression a `name` matches.
    pub pattern: Option<String>,
    /// The most characters a `name` may have.
    pub max_length: Option<usize>,
    pub summary: String,
}

pub fn example_command_line(example: &Example) -> String {
    let words: Vec<String> = example.args.iter().map(|arg| shell_word(arg)).collect();
    format!("{NAME} {}", words.join(" "))
}

fn shell_word(word: &str) -> String {
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
        }
    }

    pub fn describe(&self) -> ArgumentDescription {
        let (minimum, maximum) = match self.value {
            ValueKind::Integer { min, max } => (Some(min), max),
            _ => (None, None),
        };
        let name = self.value == ValueKind::Name;
        ArgumentDescription {
            name: self.name.to_string(),
            kind: match self.kind {
                ArgumentKind::Positional => "positional",
                ArgumentKind::Option => "option",
                ArgumentKind::Flag => "flag",
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
            pattern: name.then(|| NAME_PATTERN.to_string()),
            max_length: name.then_some(NAME_MAX_LENGTH),
            summary: self.summary.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_quote_words_a_shell_would_split() {
        let example = Example { summary: "", args: &["notes", "create", "--title", "Bob's list"] };
        assert_eq!(
            example_command_line(&example),
            format!("{NAME} notes create --title 'Bob'\\''s list'")
        );
    }
}
