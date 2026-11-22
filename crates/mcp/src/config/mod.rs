//! The servers `mcp` can reach: the user's and the project's `.mcp.json`, merged.
//!
//! `docs/configuration.md` is the contract this module implements.

mod canonical;
mod entry;
mod file;
mod interpolate;
mod server;
mod workspace;

use std::path::{Path, PathBuf};

use serde_json::json;

pub use self::entry::{Entry, TransportKind};
pub use self::file::{ConfigFile, Source};
pub use self::server::{Resolved, Server};
pub use self::workspace::{FILE_NAME, Workspace, WorkspaceReason};
use crate::NAME;
use crate::failure::Failure;

/// Everything `mcp` read to find its servers.
pub struct Configuration {
    pub workspace: Workspace,
    pub user: ConfigFile,
    pub project: Option<ConfigFile>,
    servers: Vec<Server>,
}

impl Configuration {
    /// Locates and reads both files for the working directory `cwd`. A missing or invalid file
    /// is recorded, not an error; [`Configuration::servers`] reports an invalid one.
    ///
    /// # Errors
    ///
    /// `PRECONDITION_FAILED` when neither `XDG_CONFIG_HOME` nor `HOME` locates the user file.
    pub fn load(
        cwd: &Path,
        env: &impl Fn(&str) -> Option<String>,
    ) -> Result<Configuration, Failure> {
        let user_path = user_file(env)?;
        let workspace = Workspace::locate(cwd, &user_path);
        let user = ConfigFile::read(user_path, Source::User);
        let project =
            workspace.project_file.clone().map(|path| ConfigFile::read(path, Source::Project));
        let servers = merge(&user, project.as_ref());
        Ok(Configuration { workspace, user, project, servers })
    }

    /// The user file, then the project file when there is one.
    pub fn files(&self) -> impl Iterator<Item = &ConfigFile> {
        std::iter::once(&self.user).chain(self.project.as_ref())
    }

    /// Every server, sorted by name.
    ///
    /// # Errors
    ///
    /// `PRECONDITION_FAILED` while either file is invalid or unreadable, so a broken project file
    /// never lets a user server of the same name answer in its place.
    pub fn servers(&self) -> Result<&[Server], Failure> {
        match self.files().find(|file| file.problem.is_some()) {
            Some(file) => Err(invalid_file(file)),
            None => Ok(&self.servers),
        }
    }

    /// The server called `name`.
    ///
    /// # Errors
    ///
    /// `PRECONDITION_FAILED` for an invalid file, and `NOT_FOUND` when no file declares `name`.
    pub fn server(&self, name: &str) -> Result<&Server, Failure> {
        let servers = self.servers()?;
        servers.iter().find(|server| server.name == name).ok_or_else(|| {
            let names: Vec<&str> = servers.iter().map(|server| server.name.as_str()).collect();
            Failure::not_found(format!("no server is named `{name}`"))
                .with_details(json!({
                    "server": name,
                    "servers": names,
                    "files": self.files().map(ConfigFile::display).collect::<Vec<_>>(),
                }))
                .suggest(format!("{NAME} servers list"))
        })
    }
}

fn invalid_file(file: &ConfigFile) -> Failure {
    let problem = file.problem.clone().unwrap_or_default();
    Failure::precondition(format!("{} is invalid: {problem}", file.display()))
        .with_details(json!({ "file": file.display(), "problem": problem }))
        .suggest(format!(
            "Fix {}; `{NAME} docs show docs/configuration.md` describes the format",
            file.display()
        ))
}

/// `$XDG_CONFIG_HOME/.mcp.json` when that variable is an absolute path, else
/// `~/.config/.mcp.json`.
fn user_file(env: &impl Fn(&str) -> Option<String>) -> Result<PathBuf, Failure> {
    let absolute = |key: &str| env(key).map(PathBuf::from).filter(|path| path.is_absolute());
    absolute("XDG_CONFIG_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".config")))
        .map(|directory| directory.join(FILE_NAME))
        .ok_or_else(|| {
            Failure::precondition(
                "cannot locate the user configuration file: neither XDG_CONFIG_HOME nor HOME is \
                 an absolute path",
            )
            .suggest("Set HOME, or XDG_CONFIG_HOME to an absolute directory")
        })
}

/// The user's servers with the project's replacing any of the same name, sorted by name.
fn merge(user: &ConfigFile, project: Option<&ConfigFile>) -> Vec<Server> {
    let mut servers: Vec<Server> =
        user.servers.iter().map(|(name, written)| Server::new(name, written, user, None)).collect();
    if let Some(project) = project {
        for (name, written) in &project.servers {
            let replaced = servers.iter().position(|server| &server.name == name);
            let replaces = replaced.map(|index| servers.remove(index).file);
            servers.push(Server::new(name, written, project, replaces));
        }
    }
    servers.sort_by(|left, right| left.name.cmp(&right.name));
    servers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    struct Setup {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Setup {
        fn new(user: Option<&str>, project: Option<&str>) -> Setup {
            let dir = tempfile::tempdir().unwrap();
            let root = workspace::canonical(dir.path());
            std::fs::create_dir_all(root.join("repo/.git")).unwrap();
            if let Some(user) = user {
                write(&root.join("config/.mcp.json"), user);
            }
            if let Some(project) = project {
                write(&root.join("repo/.mcp.json"), project);
            }
            Setup { _dir: dir, root }
        }

        fn load(&self) -> Configuration {
            let config = self.root.join("config").display().to_string();
            Configuration::load(&self.root.join("repo"), &|key: &str| {
                (key == "XDG_CONFIG_HOME").then(|| config.clone())
            })
            .unwrap()
        }
    }

    #[test]
    fn a_project_server_replaces_the_user_server_of_the_same_name_whole() {
        let setup = Setup::new(
            Some(
                r#"{"mcpServers": {"shared": {"command": "user-cmd", "env": {"A": "1"}}, "mine": {"command": "m"}}}"#,
            ),
            Some(r#"{"mcpServers": {"shared": {"command": "project-cmd"}}}"#),
        );
        let configuration = setup.load();
        let servers = configuration.servers().unwrap();
        let names: Vec<&str> = servers.iter().map(|server| server.name.as_str()).collect();
        assert_eq!(names, ["mine", "shared"]);
        let shared = configuration.server("shared").unwrap();
        assert_eq!(shared.source, Source::Project);
        assert_eq!(shared.target().as_deref(), Some("project-cmd"));
        assert_eq!(shared.replaces, Some(setup.root.join("config/.mcp.json")));
        assert!(matches!(&shared.entry, Ok(Entry::Stdio { env, .. }) if env.is_empty()));
    }

    #[test]
    fn an_invalid_file_blocks_every_server() {
        let setup = Setup::new(Some(r#"{"mcpServers": {"a": {"command": "a"}}}"#), Some("{"));
        let configuration = setup.load();
        let failure = configuration.servers().unwrap_err();
        assert_eq!(failure.code, crate::failure::ErrorCode::PreconditionFailed);
        assert_eq!(
            failure.details["file"],
            setup.root.join("repo/.mcp.json").display().to_string()
        );
        assert_eq!(configuration.server("a").unwrap_err().code, failure.code);
    }

    #[test]
    fn an_unknown_server_is_not_found_and_lists_the_others() {
        let setup = Setup::new(Some(r#"{"mcpServers": {"a": {"command": "a"}}}"#), None);
        let failure = setup.load().server("b").unwrap_err();
        assert_eq!(failure.code, crate::failure::ErrorCode::NotFound);
        assert_eq!(failure.details["servers"], json!(["a"]));
    }

    #[test]
    fn the_user_file_defaults_to_the_home_config_directory() {
        let path = user_file(&|key: &str| (key == "HOME").then(|| "/home/me".to_string())).unwrap();
        assert_eq!(path, PathBuf::from("/home/me/.config/.mcp.json"));
        let relative = |key: &str| match key {
            "XDG_CONFIG_HOME" => Some("relative".to_string()),
            "HOME" => Some("/home/me".to_string()),
            _ => None,
        };
        assert_eq!(user_file(&relative).unwrap(), PathBuf::from("/home/me/.config/.mcp.json"));
        assert!(user_file(&|_: &str| None).is_err());
    }
}
