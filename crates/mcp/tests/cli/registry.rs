//! The registry is the one declaration of every command; these tests fail when the parser,
//! the runtime defaults, or the examples drift from it.

use std::collections::BTreeSet;

use clap::Command;
use mcp::parser;
use mcp::registry::{self, ArgumentKind, CommandSpec, Effect, ValueKind};

use crate::support::{Fixture, assert_matches_output_schema};

fn leaves(command: &Command, prefix: &[String], found: &mut Vec<(Vec<String>, Command)>) {
    if !command.has_subcommands() {
        found.push((prefix.to_vec(), command.clone()));
        return;
    }
    for sub in command.get_subcommands() {
        let mut path = prefix.to_vec();
        path.push(sub.get_name().to_string());
        leaves(sub, &path, found);
    }
}

#[test]
fn every_entry_is_complete_and_uniquely_identified() {
    let mut ids = BTreeSet::new();
    for command in registry::all() {
        assert_eq!(command.id, command.path.join("."), "{} is not its path joined", command.id);
        assert!(ids.insert(command.id), "{} is registered twice", command.id);
        assert!(!command.summary.is_empty() && !command.description.is_empty(), "{}", command.id);
        assert!(!command.examples.is_empty(), "{} has no example", command.id);
        assert!(
            registry::RESOURCES.iter().any(|(resource, _)| *resource == command.path[0]),
            "{} belongs to no resource in RESOURCES",
            command.id
        );
        let modifies = matches!(command.effect, Effect::Modifies(_));
        assert_eq!(
            modifies, command.dry_run,
            "{}: every modifying command, and only those, supports --dry-run",
            command.id
        );
        let declares_dry_run = command.arguments.iter().any(|argument| argument.name == "dry-run");
        assert_eq!(command.dry_run, declares_dry_run, "{}: --dry-run declaration", command.id);
        if modifies {
            assert!(
                command.examples.iter().any(|example| example.args.contains(&"--dry-run")),
                "{} has no --dry-run example",
                command.id
            );
        }
        for argument in command.arguments {
            let kebab = argument
                .name
                .chars()
                .all(|character| character.is_ascii_lowercase() || character == '-');
            assert!(kebab, "{}: argument `{}` is not kebab-case", command.id, argument.name);
        }
    }
}

/// The arguments a parser command defines itself, without Clap's `--help` and `--version`.
fn parsed_arguments(command: &Command) -> BTreeSet<&str> {
    command
        .get_arguments()
        .map(|argument| argument.get_id().as_str())
        .filter(|name| *name != "help" && *name != "version")
        .collect()
}

#[test]
fn every_parser_command_and_argument_is_in_the_registry() {
    let root = parser::build();
    let globals: BTreeSet<&str> =
        registry::GLOBAL_ARGUMENTS.iter().map(|argument| argument.name).collect();
    assert_eq!(
        parsed_arguments(&root),
        globals,
        "the root parses other options than GLOBAL_ARGUMENTS"
    );

    let mut found = Vec::new();
    leaves(&root, &[], &mut found);
    let parsed: BTreeSet<String> = found.iter().map(|(path, _)| path.join(".")).collect();
    let declared: BTreeSet<String> =
        registry::all().iter().map(|command| command.id.to_string()).collect();
    assert_eq!(parsed, declared, "the parser's commands and the registry's differ");

    for (path, command) in &found {
        let words: Vec<&str> = path.iter().map(String::as_str).collect();
        let spec = registry::find(&words).expect("checked above");
        let declared: BTreeSet<&str> =
            spec.arguments.iter().map(|argument| argument.name).collect();
        assert_eq!(
            parsed_arguments(command),
            declared,
            "`{}` parses other arguments than it declares",
            spec.command_line()
        );
    }
}

#[test]
fn every_example_parses_as_its_own_command() {
    for command in registry::all() {
        for example in command.examples {
            let mut args = vec![mcp::NAME];
            args.extend_from_slice(example.args);
            let result = parser::build().try_get_matches_from(&args);
            assert!(result.is_ok(), "`{}` does not parse: {}", args.join(" "), result.unwrap_err());
            assert!(
                example.args.starts_with(command.path),
                "example `{}` belongs to another command than {}",
                args.join(" "),
                command.id
            );
        }
    }
}

/// The smallest command line that parses: the path plus a value for each required argument.
fn minimal_args(command: &CommandSpec) -> Vec<String> {
    let mut args: Vec<String> = std::iter::once(mcp::NAME)
        .chain(command.path.iter().copied())
        .map(str::to_string)
        .collect();
    for argument in command.arguments.iter().filter(|argument| argument.required) {
        let value = match argument.value {
            ValueKind::Choice(values) => values[0].to_string(),
            ValueKind::Integer { min, .. } => min.to_string(),
            _ => "placeholder".to_string(),
        };
        match argument.kind {
            ArgumentKind::Positional => args.push(value),
            ArgumentKind::Option => args.extend([format!("--{}", argument.name), value]),
            ArgumentKind::Flag => args.push(format!("--{}", argument.name)),
        }
    }
    args
}

#[test]
fn every_declared_default_is_the_runtime_default() {
    for command in registry::all() {
        let args = minimal_args(command);
        let matches = parser::build()
            .try_get_matches_from(&args)
            .unwrap_or_else(|error| panic!("`{}` does not parse: {error}", args.join(" ")));
        let mut leaf = &matches;
        while let Some((_, sub)) = leaf.subcommand() {
            leaf = sub;
        }
        for argument in command.all_arguments().filter(|argument| !argument.required) {
            if argument.kind == ArgumentKind::Flag {
                assert_eq!(argument.default, None, "{}: a flag declares a default", command.id);
                continue;
            }
            let runtime: Option<String> = leaf
                .get_raw(argument.name)
                .and_then(|mut values| values.next())
                .map(|value| value.to_string_lossy().into_owned());
            assert_eq!(
                runtime.as_deref(),
                argument.default,
                "{}: --{} defaults differently at run time",
                command.id,
                argument.name
            );
        }
    }
}

#[test]
fn every_example_runs_and_its_output_matches_its_schema() {
    for command in registry::all() {
        for example in command.examples {
            let fixture = Fixture::seeded();
            let outcome = fixture.run(example.args);
            let shown = registry::example_command_line(example);
            assert_eq!(
                outcome.status, 0,
                "`{shown}` failed:\nstdout: {}\nstderr: {}",
                outcome.stdout, outcome.stderr
            );
            assert_matches_output_schema(command.path, &outcome.json());
        }
    }
}
