//! The Clap parser, generated from the registry.

use clap::builder::{PossibleValuesParser, RangedU64ValueParser};
use clap::{Arg, ArgAction, Command};

use crate::NAME;
use crate::registry::{
    self, ArgumentKind, ArgumentSpec, CommandSpec, GLOBAL_ARGUMENTS, NAME_MAX_LENGTH, RESOURCES,
    ValueKind,
};

const ORIENTATION: &str = "\
MCP CLI keeps notes in a local SQLite store. Every command can be discovered, inspected, and \
previewed from this binary; no network or credentials are involved.

Results are JSON on stdout; diagnostics and structured errors go to stderr. Pass --output text \
for a human rendering.";

const DISCOVERY: &str = "\
Discover before you run:
  mcp cli search \"<task>\"            find the command for a task
  mcp schema <resource> <operation>  arguments, defaults, effects, errors, examples
  mcp docs search \"<topic>\"          search the concepts and the CLI reference
  mcp context show                   the store commands read and write

Modifying commands accept --dry-run. No command prompts.";

pub fn build() -> Command {
    let mut root = Command::new(NAME)
        .version(env!("CARGO_PKG_VERSION"))
        .about(ORIENTATION)
        .after_help(DISCOVERY)
        .subcommand_required(true)
        .disable_help_subcommand(true);
    for argument in GLOBAL_ARGUMENTS {
        root = root.arg(clap_arg(argument).global(true));
    }

    let commands = registry::all();
    for (resource, summary) in RESOURCES {
        let members: Vec<&CommandSpec> =
            commands.iter().copied().filter(|command| command.path[0] == *resource).collect();
        root = match members.as_slice() {
            [] => root,
            [single] if single.path.len() == 1 => root.subcommand(leaf(single)),
            _ => {
                let mut group = Command::new(*resource)
                    .about(*summary)
                    .subcommand_required(true)
                    .disable_help_subcommand(true);
                for member in members {
                    group = group.subcommand(leaf(member));
                }
                root.subcommand(group)
            }
        };
    }
    root
}

fn leaf(spec: &CommandSpec) -> Command {
    let name = *spec.path.last().expect("a command path has at least one word");
    let mut command = Command::new(name)
        .about(spec.summary)
        .long_about(format!("{}\n\n{}", spec.summary, spec.description))
        .after_help(examples_help(spec));
    for argument in spec.arguments {
        command = command.arg(clap_arg(argument));
    }
    command
}

fn examples_help(spec: &CommandSpec) -> String {
    let mut help = format!("Effect: {}\n\nExamples:", spec.effect_label());
    for example in spec.examples {
        help.push_str(&format!(
            "\n  {}\n      {}",
            registry::example_command_line(example),
            example.summary
        ));
    }
    help
}

fn clap_arg(spec: &ArgumentSpec) -> Arg {
    let mut arg = Arg::new(spec.name).help(spec.summary);
    arg = match spec.kind {
        ArgumentKind::Positional => arg.value_name(spec.name.to_uppercase()),
        ArgumentKind::Option => {
            arg.long(spec.name).value_name(spec.value.type_name().to_uppercase())
        }
        ArgumentKind::Flag => arg.long(spec.name),
    };
    arg = match spec.value {
        ValueKind::Boolean => arg.action(ArgAction::SetTrue),
        ValueKind::Integer { min, max } => arg
            .value_parser(RangedU64ValueParser::<u64>::new().range(min..=max.unwrap_or(u64::MAX))),
        ValueKind::Name => arg.value_parser(parse_name),
        ValueKind::Choice(values) => {
            arg.value_parser(PossibleValuesParser::new(values.iter().copied()))
        }
        ValueKind::String | ValueKind::Path | ValueKind::Text => arg,
    };
    if spec.kind != ArgumentKind::Flag {
        arg = arg.required(spec.required);
        if spec.multiple {
            arg = match spec.kind {
                ArgumentKind::Positional => arg.num_args(if spec.required { 1.. } else { 0.. }),
                _ => arg.action(ArgAction::Append),
            };
        }
        if let Some(default) = spec.default {
            arg = arg.default_value(default);
        }
    }
    arg
}

/// Accepts a note name: see [`registry::NAME_PATTERN`].
fn parse_name(value: &str) -> Result<String, String> {
    if is_name(value) {
        Ok(value.to_string())
    } else {
        Err(format!(
            "use 1 to {NAME_MAX_LENGTH} lowercase letters and digits in hyphen-separated words, \
             such as `groceries` or `trip-2027`"
        ))
    }
}

/// Whether `value` matches [`registry::NAME_PATTERN`] and is at most [`NAME_MAX_LENGTH`] long.
pub fn is_name(value: &str) -> bool {
    value.len() <= NAME_MAX_LENGTH
        && value.split('-').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generated_parser_is_well_formed() {
        build().debug_assert();
    }

    #[test]
    fn names_are_hyphenated_lowercase_words() {
        for good in ["groceries", "trip-2027", "a", "0-1-2"] {
            assert!(is_name(good), "{good}");
        }
        let long = "a".repeat(NAME_MAX_LENGTH + 1);
        for bad in ["", "-a", "a-", "a--b", "Groceries", "a_b", "a b", "é", long.as_str()] {
            assert!(!is_name(bad), "{bad}");
        }
    }
}
