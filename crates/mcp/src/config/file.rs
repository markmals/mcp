//! Reading one `.mcp.json` file.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

/// Which configuration file a server comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// The nearest `.mcp.json` of the working directory's project.
    Project,
    /// `$XDG_CONFIG_HOME/.mcp.json`, which applies in every directory.
    User,
}

/// One configuration file and what it holds.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    pub path: PathBuf,
    pub source: Source,
    pub exists: bool,
    /// Why the file is invalid or unreadable; `None` for a valid or absent file.
    pub problem: Option<String>,
    /// Its servers as written, in file order.
    pub servers: Vec<(String, Value)>,
}

impl ConfigFile {
    pub fn read(path: PathBuf, source: Source) -> ConfigFile {
        let mut file =
            ConfigFile { path, source, exists: false, problem: None, servers: Vec::new() };
        match std::fs::read_to_string(&file.path) {
            Ok(text) => {
                file.exists = true;
                match parse(&text) {
                    Ok(servers) => file.servers = servers,
                    Err(problem) => file.problem = Some(problem),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                file.exists = file.path.exists();
                file.problem = Some(format!("cannot read it: {error}"));
            }
        }
        file
    }

    pub fn display(&self) -> String {
        display(&self.path)
    }
}

pub(super) fn display(path: &Path) -> String {
    path.display().to_string()
}

fn parse(text: &str) -> Result<Vec<(String, Value)>, String> {
    let root: Value =
        serde_json::from_str(text).map_err(|error| format!("invalid JSON: {error}"))?;
    let Value::Object(root) = root else {
        return Err("the top level must be a JSON object".into());
    };
    if let Some(key) = root.keys().find(|key| *key != "mcpServers" && *key != "$schema") {
        return Err(format!(
            "unknown top-level key `{key}`; a file holds only `mcpServers` and `$schema`"
        ));
    }
    match root.get("mcpServers") {
        Some(Value::Object(servers)) => {
            Ok(servers.iter().map(|(name, entry)| (name.clone(), entry.clone())).collect())
        }
        Some(_) => Err("`mcpServers` must be a JSON object".into()),
        None => Err("it has no `mcpServers` object".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_servers_in_file_order() {
        let servers = parse(r#"{"$schema": "x", "mcpServers": {"b": {}, "a": {}}}"#).unwrap();
        let names: Vec<&str> = servers.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["b", "a"]);
    }

    #[test]
    fn rejects_anything_but_an_mcp_servers_object() {
        for (text, expected) in [
            ("{", "invalid JSON"),
            ("[]", "top level"),
            (r#"{"servers": {}}"#, "unknown top-level key `servers`"),
            (r#"{"mcpServers": []}"#, "must be a JSON object"),
            ("{}", "no `mcpServers`"),
        ] {
            let problem = parse(text).unwrap_err();
            assert!(problem.contains(expected), "{text}: {problem}");
        }
    }

    #[test]
    fn an_absent_file_is_not_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let file = ConfigFile::read(dir.path().join(".mcp.json"), Source::User);
        assert!(!file.exists && file.problem.is_none() && file.servers.is_empty());
    }
}
