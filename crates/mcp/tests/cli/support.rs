//! Runs the real `mcp` binary against a temporary store.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::SystemTime;

use serde_json::Value;
use tempfile::TempDir;

/// A temporary directory holding the store, with every location variable pointed inside it,
/// so no test reads or writes the developer's data.
pub(crate) struct Fixture {
    dir: TempDir,
    env: Vec<(String, String)>,
}

/// What one run of `mcp` produced.
#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) status: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Outcome {
    /// Waits for a run that [`Fixture::start`] began.
    pub(crate) fn of(child: Child) -> Outcome {
        let output = child.wait_with_output().expect("mcp finishes");
        Outcome {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// The result on stdout, which must be one JSON document.
    pub(crate) fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|error| {
            panic!("stdout is not JSON ({error}):\n{}\nstderr:\n{}", self.stdout, self.stderr)
        })
    }

    /// The `error` object on stderr, after checking the whole document against its schema.
    pub(crate) fn error(&self) -> Value {
        let document: Value = serde_json::from_str(self.stderr.trim()).unwrap_or_else(|error| {
            panic!("stderr is not one JSON document ({error}):\n{}", self.stderr)
        });
        assert_matches_schema(&error_schema(), &document, "the error document");
        document["error"].clone()
    }

    /// Asserts a failure: the exit status, the code, and nothing on stdout.
    pub(crate) fn assert_failed(&self, status: i32, code: &str) -> Value {
        assert_eq!(self.status, status, "stdout: {}\nstderr: {}", self.stdout, self.stderr);
        assert!(self.stdout.is_empty(), "a failure wrote to stdout: {}", self.stdout);
        let error = self.error();
        assert_eq!(error["code"], code, "{error}");
        error
    }

    /// Asserts success with one JSON document on stdout and nothing on stderr.
    pub(crate) fn success(&self) -> Value {
        assert_eq!(self.status, 0, "stdout: {}\nstderr: {}", self.stdout, self.stderr);
        assert!(self.stderr.is_empty(), "a success wrote to stderr: {}", self.stderr);
        self.json()
    }
}

/// Every file and directory under a root: contents and modification time for files.
type Snapshot = BTreeMap<PathBuf, Option<(Vec<u8>, SystemTime)>>;

impl Fixture {
    /// An empty directory; `MCP_STORE` names `store.sqlite3` inside it, which does not exist.
    pub(crate) fn new() -> Fixture {
        let dir = tempfile::Builder::new().prefix("mcp-test-").tempdir().expect("a temp dir");
        let store = dir.path().join("store.sqlite3").display().to_string();
        let data = dir.path().join("data").display().to_string();
        Fixture {
            env: vec![
                ("MCP_STORE".into(), store),
                ("XDG_DATA_HOME".into(), data.clone()),
                ("LOCALAPPDATA".into(), data),
            ],
            dir,
        }
    }

    /// A store holding the notes `groceries` and `standup`, both at revision 1, which the
    /// registered examples use.
    pub(crate) fn seeded() -> Fixture {
        let fixture = Fixture::new();
        for (name, title, body) in [
            ("groceries", "Groceries", "Milk, eggs"),
            ("standup", "Standup notes", "Shipped the parser"),
        ] {
            fixture
                .run(&["notes", "create", "--name", name, "--title", title, "--body", body])
                .success();
        }
        fixture
    }

    pub(crate) fn root(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.root().join(relative)
    }

    pub(crate) fn store(&self) -> PathBuf {
        self.path("store.sqlite3")
    }

    /// Sets an environment variable for later runs; `None` removes it.
    pub(crate) fn set_env(&mut self, key: &str, value: Option<&str>) {
        self.env.retain(|(name, _)| name != key);
        if let Some(value) = value {
            self.env.push((key.to_string(), value.to_string()));
        }
    }

    pub(crate) fn run(&self, args: &[&str]) -> Outcome {
        self.run_with_stdin(args, "")
    }

    pub(crate) fn run_with_stdin(&self, args: &[&str], stdin: &str) -> Outcome {
        let mut child = self.start(args);
        child.stdin.take().expect("piped stdin").write_all(stdin.as_bytes()).expect("stdin");
        Outcome::of(child)
    }

    /// Starts `mcp` with piped standard streams and returns while it runs; it reads stdin
    /// until the caller closes it.
    pub(crate) fn start(&self, args: &[&str]) -> Child {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcp"));
        command
            .args(args)
            .current_dir(self.root())
            .env_remove("MCP_STORE")
            .env_remove("XDG_DATA_HOME")
            .env_remove("LOCALAPPDATA")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command.spawn().expect("mcp runs")
    }

    /// Every file and directory under the fixture, to prove a command changed nothing.
    pub(crate) fn snapshot(&self) -> Snapshot {
        fn walk(directory: &Path, entries: &mut Snapshot) {
            for entry in std::fs::read_dir(directory).expect("a readable directory") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    entries.insert(path.clone(), None);
                    walk(&path, entries);
                } else {
                    let contents = std::fs::read(&path).expect("a readable file");
                    let modified = std::fs::metadata(&path)
                        .and_then(|metadata| metadata.modified())
                        .expect("a modification time");
                    entries.insert(path, Some((contents, modified)));
                }
            }
        }
        let mut entries = BTreeMap::new();
        walk(self.root(), &mut entries);
        entries
    }
}

/// Fails the test when `result` does not match the output schema `mcp schema` declares for
/// the command at `path`.
pub(crate) fn assert_matches_output_schema(path: &[&str], result: &Value) {
    let mut args = vec!["schema"];
    args.extend_from_slice(path);
    let description = Fixture::new().run(&args).success();
    assert_matches_schema(&description["output"], result, &format!("mcp {}", path.join(" ")));
}

fn error_schema() -> Value {
    schemars::schema_for!(mcp::failure::ErrorDocument).to_value()
}

fn assert_matches_schema(schema: &Value, instance: &Value, what: &str) {
    let validator = jsonschema::validator_for(schema)
        .unwrap_or_else(|error| panic!("{what} has an invalid schema: {error}"));
    let problems: Vec<String> = validator
        .iter_errors(instance)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect();
    assert!(
        problems.is_empty(),
        "{what} does not match its schema:\n{}\n{}",
        problems.join("\n"),
        serde_json::to_string_pretty(instance).unwrap_or_default()
    );
}
