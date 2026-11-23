//! One configured server: its entry as written, where it came from, and how to reach it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::canonical::fingerprint;
use super::entry::{Entry, EntryProblem};
use super::file::{ConfigFile, Source, display};
use super::interpolate::expand;
use crate::NAME;
use crate::daemon::protocol::Transport;
use crate::failure::Failure;
use crate::registry::shell_word;

/// Interpolated and literal secret values shorter than this are left in output: redacting
/// every `1` or `true` would destroy messages without protecting anything.
const SECRET_MIN_LENGTH: usize = 8;

#[derive(Debug, Clone)]
pub struct Server {
    pub name: String,
    pub source: Source,
    pub file: PathBuf,
    /// The entry exactly as the file has it.
    pub written: Value,
    pub fingerprint: String,
    /// The user file whose server of the same name this project server replaces.
    pub replaces: Option<PathBuf>,
    pub entry: Result<Entry, EntryProblem>,
}

/// A server's connection settings with every `${VAR}` expanded, and the values to keep out of
/// output.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub transport: Transport,
    pub secrets: Vec<String>,
}

impl Server {
    pub fn new(
        name: &str,
        written: &Value,
        file: &ConfigFile,
        replaces: Option<PathBuf>,
    ) -> Server {
        Server {
            name: name.to_string(),
            source: file.source,
            file: file.path.clone(),
            written: written.clone(),
            fingerprint: fingerprint(written),
            replaces,
            entry: Entry::parse(name, written),
        }
    }

    /// The command line or URL as written, for people to recognize the server by.
    pub fn target(&self) -> Option<String> {
        if let Some(url) = self.written.get("url").and_then(Value::as_str) {
            return Some(url.to_string());
        }
        let command = self.written.get("command").and_then(Value::as_str)?;
        let args = self.written.get("args").and_then(Value::as_array);
        let words = std::iter::once(command)
            .chain(args.into_iter().flatten().filter_map(Value::as_str))
            .map(shell_word);
        Some(words.collect::<Vec<_>>().join(" "))
    }

    /// The names of the configured `env` or `headers`, never their values.
    pub fn variable_names(&self, field: &str) -> Vec<String> {
        self.written
            .get(field)
            .and_then(Value::as_object)
            .map(|entries| entries.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Why the entry is unusable, as the failure a command reports; `None` for a usable entry.
    pub fn problem(&self) -> Option<Failure> {
        self.entry.as_ref().err().map(|problem| self.entry_failure(problem))
    }

    fn entry_failure(&self, problem: &EntryProblem) -> Failure {
        Failure::precondition(format!(
            "server `{}` in {} is unusable: {}",
            self.name,
            display(&self.file),
            problem.message
        ))
        .with_details(json!({
            "server": self.name,
            "file": display(&self.file),
            "field": problem.field,
        }))
        .suggest(format!(
            "Fix the entry in {}; `{NAME} docs show docs/configuration.md` lists the fields mcp \
             supports",
            display(&self.file)
        ))
    }

    /// Expands the entry's variables from `env` for a connection from `workspace`.
    ///
    /// # Errors
    ///
    /// `PRECONDITION_FAILED` for an unusable entry, a variable that is unset and has no default
    /// (named, its value never shown), or a URL that is not absolute `http` or `https`.
    pub fn resolve(
        &self,
        workspace: &Path,
        env: &impl Fn(&str) -> Option<String>,
    ) -> Result<Resolved, Failure> {
        let entry = self.entry.as_ref().map_err(|problem| self.entry_failure(problem))?;
        let mut secrets = Vec::new();
        let mut expand_value = |value: &str, whole_is_secret: bool| {
            let expanded = expand(value, env).map_err(|variable| self.unset(&variable))?;
            secrets.extend(expanded.substituted);
            if whole_is_secret {
                secrets.push(expanded.text.clone());
            }
            Ok::<String, Failure>(expanded.text)
        };
        let transport = match entry {
            Entry::Stdio { command, args, env: variables } => {
                let command = expand_value(command, false)?;
                let command = if command.contains('/') && Path::new(&command).is_relative() {
                    workspace.join(&command).display().to_string()
                } else {
                    command
                };
                let args =
                    args.iter().map(|arg| expand_value(arg, false)).collect::<Result<_, _>>()?;
                let env = expand_map(variables, &mut expand_value)?;
                Transport::Stdio { command, args, env, inherited: BTreeMap::new() }
            }
            Entry::Http { url, headers } => {
                let url = expand_value(url, false)?;
                if !is_http_url(&url) {
                    return Err(self.bad_url());
                }
                let headers = expand_map(headers, &mut expand_value)?;
                Transport::Http { url, headers }
            }
        };
        secrets.retain(|secret| secret.chars().count() >= SECRET_MIN_LENGTH);
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets.dedup();
        Ok(Resolved { transport, secrets })
    }

    fn unset(&self, variable: &str) -> Failure {
        Failure::precondition(format!(
            "server `{}` in {} uses ${{{variable}}}, which is unset and has no default",
            self.name,
            display(&self.file)
        ))
        .with_details(json!({ "server": self.name, "file": display(&self.file), "variable": variable }))
        .suggest(format!(
            "Set {variable} in the environment mcp runs in, or give it a default: ${{{variable}:-<value>}}"
        ))
    }

    fn bad_url(&self) -> Failure {
        Failure::precondition(format!(
            "server `{}` in {} has a `url` that is not an absolute http or https URL",
            self.name,
            display(&self.file)
        ))
        .with_details(json!({ "server": self.name, "file": display(&self.file), "field": "url" }))
    }
}

fn expand_map(
    entries: &BTreeMap<String, String>,
    expand_value: &mut impl FnMut(&str, bool) -> Result<String, Failure>,
) -> Result<BTreeMap<String, String>, Failure> {
    entries.iter().map(|(name, value)| Ok((name.clone(), expand_value(value, true)?))).collect()
}

fn is_http_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https")
            && parsed.host_str().is_some_and(|host| !host.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::failure::ErrorCode;

    fn server(written: Value) -> Server {
        let file = ConfigFile {
            path: PathBuf::from("/repo/.mcp.json"),
            source: Source::Project,
            exists: true,
            problem: None,
            servers: Vec::new(),
        };
        Server::new("demo", &written, &file, None)
    }

    fn env(key: &str) -> Option<String> {
        match key {
            "TOKEN" => Some("a-long-secret-token".into()),
            "SHORT" => Some("abc".into()),
            "HOST" => Some("example.com".into()),
            _ => None,
        }
    }

    #[test]
    fn expands_variables_and_collects_the_values_to_redact() {
        let resolved = server(json!({
            "command": "./bin/server",
            "args": ["--token=${TOKEN}", "${SHORT}"],
            "env": {"LITERAL": "plain-literal-value", "TINY": "1"}
        }))
        .resolve(Path::new("/repo"), &env)
        .unwrap();
        let Transport::Stdio { command, args, env, .. } = resolved.transport else { panic!() };
        assert_eq!(command, "/repo/./bin/server");
        assert_eq!(args, ["--token=a-long-secret-token", "abc"]);
        assert_eq!(env["LITERAL"], "plain-literal-value");
        assert_eq!(resolved.secrets, ["a-long-secret-token", "plain-literal-value"]);
    }

    #[test]
    fn a_bare_command_name_is_left_for_path_lookup() {
        let resolved = server(json!({"command": "npx"})).resolve(Path::new("/repo"), &env).unwrap();
        assert!(matches!(resolved.transport, Transport::Stdio { command, .. } if command == "npx"));
    }

    #[test]
    fn an_unset_variable_is_named_and_its_server_unusable() {
        let failure = server(json!({"url": "https://${HOST}/${MISSING}"}))
            .resolve(Path::new("/repo"), &env)
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::PreconditionFailed);
        assert_eq!(failure.details["variable"], "MISSING");
    }

    #[test]
    fn a_url_must_be_absolute_http() {
        for url in
            ["ftp://x", "localhost:3000/mcp", "http://", "${HOST}/mcp", "http://bad host/mcp"]
        {
            let failure = server(json!({"url": url})).resolve(Path::new("/r"), &env).unwrap_err();
            assert_eq!(failure.details["field"], "url", "{url}");
        }
        assert!(
            server(json!({"url": "https://${HOST}/mcp"})).resolve(Path::new("/r"), &env).is_ok()
        );
    }

    #[test]
    fn the_target_is_the_command_line_or_url_as_written() {
        let stdio = server(json!({"command": "npx", "args": ["-y", "pkg@latest", "a b"]}));
        assert_eq!(stdio.target().as_deref(), Some("npx -y pkg@latest 'a b'"));
        let http = server(json!({"url": "https://${HOST}/mcp", "headers": {"Authorization": "x"}}));
        assert_eq!(http.target().as_deref(), Some("https://${HOST}/mcp"));
        assert_eq!(http.variable_names("headers"), ["Authorization"]);
    }
}
