//! A parsed command line, as a command's `run` function sees it.

use std::io::Read;
use std::path::PathBuf;

use clap::ArgMatches;
use serde_json::json;

use crate::failure::Failure;
use crate::registry::CommandSpec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Json,
    Text,
}

pub struct Invocation<'a> {
    pub spec: &'static CommandSpec,
    pub output: OutputFormat,
    pub cwd: PathBuf,
    matches: &'a ArgMatches,
}

impl<'a> Invocation<'a> {
    pub fn new(
        spec: &'static CommandSpec,
        matches: &'a ArgMatches,
        output: OutputFormat,
        cwd: PathBuf,
    ) -> Invocation<'a> {
        Invocation { spec, output, cwd, matches }
    }

    pub fn string(&self, name: &str) -> Option<String> {
        self.matches.get_one::<String>(name).cloned()
    }

    pub fn strings(&self, name: &str) -> Vec<String> {
        self.matches
            .get_many::<String>(name)
            .map(|values| values.cloned().collect())
            .unwrap_or_default()
    }

    /// An integer argument; `None` when it is absent or the command does not declare it, as
    /// `--timeout` on a command that never contacts a server.
    pub fn integer(&self, name: &str) -> Option<u64> {
        self.matches.try_get_one::<u64>(name).ok().flatten().copied()
    }

    pub fn flag(&self, name: &str) -> bool {
        self.matches.get_flag(name)
    }

    pub fn dry_run(&self) -> bool {
        self.spec.dry_run && self.flag("dry-run")
    }

    /// A text argument: the text itself, `@<path>` to read a file, or `-` to read stdin.
    ///
    /// # Errors
    ///
    /// `INVALID_ARGUMENT` when the file or stdin cannot be read as UTF-8 text.
    pub fn text(&self, name: &str) -> Result<Option<String>, Failure> {
        let Some(value) = self.string(name) else {
            return Ok(None);
        };
        if value == "-" {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).map_err(|error| {
                Failure::invalid_argument(format!("cannot read --{name} from stdin: {error}"))
                    .with_details(json!({ "argument": name, "value": "-" }))
            })?;
            return Ok(Some(text));
        }
        if let Some(path) = value.strip_prefix('@') {
            let path = self.cwd.join(path);
            return std::fs::read_to_string(&path).map(Some).map_err(|error| {
                Failure::invalid_argument(format!(
                    "cannot read --{name} from {}: {error}",
                    path.display()
                ))
                .with_details(json!({ "argument": name, "path": path.display().to_string() }))
            });
        }
        Ok(Some(value))
    }

    /// An environment variable of this invocation; a value that is not UTF-8 counts as unset.
    pub fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    /// The whole environment of this invocation, which a stdio server it starts inherits.
    pub fn environment(&self) -> std::collections::BTreeMap<String, String> {
        std::env::vars().collect()
    }
}
