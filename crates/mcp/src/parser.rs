//! The Clap parser, generated from the registry.

use std::ffi::OsString;

use clap::builder::{PossibleValuesParser, RangedU64ValueParser};
use clap::{Arg, ArgAction, Command};

use crate::NAME;
use crate::registry::{
    self, ArgumentKind, ArgumentSpec, CommandSpec, GLOBAL_ARGUMENTS, RESOURCES, ValueKind,
};

const ORIENTATION: &str = "\
mcp calls the tools of the MCP servers configured in a project's .mcp.json and in \
$XDG_CONFIG_HOME/.mcp.json. Every command can be discovered, inspected, and previewed from this \
binary.

Results are JSON on stdout; diagnostics and structured errors go to stderr. Pass --output text \
for a human rendering.";

const DISCOVERY: &str = "\
Discover before you run:
  mcp cli search \"<task>\"            find the command for a task
  mcp schema <resource> <operation>  arguments, defaults, effects, errors, examples
  mcp docs search \"<topic>\"          search the concepts and the CLI reference
  mcp context show                   the configuration files, workspace, and daemon in effect

Then: mcp servers list, mcp tools search \"<task>\", mcp tools info <server> <tool>,
mcp call <server> <tool> --<property> <value>.

Commands that change something accept --dry-run. No command prompts.";

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

/// `args` with a `--` inserted after the positionals of a command that takes trailing words,
/// so every word after them reaches the command unparsed: `mcp call demo echo --output x`
/// gives the tool `--output x`, while `mcp call --output text demo echo` sets mcp's own option.
pub fn separate_trailing(args: &[OsString]) -> Vec<OsString> {
    let words: Vec<String> = args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
    let Some((start, spec)) = (1..words.len()).find_map(|end| {
        let path: Vec<&str> = words[1..=end].iter().map(String::as_str).collect();
        registry::find(&path).map(|spec| (end + 1, spec))
    }) else {
        return args.to_vec();
    };
    if !spec.arguments.iter().any(|argument| argument.kind == ArgumentKind::Trailing) {
        return args.to_vec();
    }
    let positionals =
        spec.arguments.iter().filter(|argument| argument.kind == ArgumentKind::Positional).count();
    let mut seen = 0;
    let mut index = start;
    while index < words.len() && seen < positionals {
        let word = &words[index];
        index += 1;
        if word == "--" {
            return args.to_vec();
        }
        match word.strip_prefix("--") {
            Some(name) if !name.contains('=') => {
                let takes_value = spec
                    .all_arguments()
                    .any(|argument| argument.name == name && argument.kind == ArgumentKind::Option);
                if takes_value {
                    index += 1;
                }
            }
            Some(_) => {}
            None => seen += 1,
        }
    }
    if seen < positionals {
        return args.to_vec();
    }
    let mut separated = args[..index.min(args.len())].to_vec();
    separated.push("--".into());
    separated.extend_from_slice(&args[index.min(args.len())..]);
    separated
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
        ArgumentKind::Trailing => {
            return arg
                .value_name("--PROPERTY VALUE")
                .num_args(0..)
                .last(true)
                .allow_hyphen_values(true);
        }
    };
    arg = match spec.value {
        ValueKind::Boolean => arg.action(ArgAction::SetTrue),
        ValueKind::Integer { min, max } => arg
            .value_parser(RangedU64ValueParser::<u64>::new().range(min..=max.unwrap_or(u64::MAX))),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn separated(words: &[&str]) -> Vec<String> {
        let args: Vec<OsString> = words.iter().map(OsString::from).collect();
        separate_trailing(&args).iter().map(|arg| arg.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn the_generated_parser_is_well_formed() {
        build().debug_assert();
    }

    #[test]
    fn words_after_the_tool_belong_to_the_tool() {
        assert_eq!(
            separated(&["mcp", "call", "--output", "text", "demo", "echo", "--output", "x"]),
            ["mcp", "call", "--output", "text", "demo", "echo", "--", "--output", "x"]
        );
        assert_eq!(
            separated(&["mcp", "call", "--dry-run", "--timeout=5", "demo", "echo"]),
            ["mcp", "call", "--dry-run", "--timeout=5", "demo", "echo", "--"]
        );
        assert_eq!(separated(&["mcp", "call", "demo"]), ["mcp", "call", "demo"]);
        assert_eq!(separated(&["mcp", "tools", "list", "--x"]), ["mcp", "tools", "list", "--x"]);
    }

    #[test]
    fn separated_words_parse_as_the_tool_s_arguments() {
        let args: Vec<OsString> = ["mcp", "call", "demo", "echo", "--help", "--dry-run"]
            .iter()
            .map(OsString::from)
            .collect();
        let matches = build().try_get_matches_from(separate_trailing(&args)).unwrap();
        let (_, call) = matches.subcommand().unwrap();
        assert!(!call.get_flag("dry-run"));
        let trailing: Vec<&String> = call.get_many::<String>("arguments").unwrap().collect();
        assert_eq!(trailing, ["--help", "--dry-run"]);
    }
}
