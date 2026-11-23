//! Runs the real `mcp` binary in a temporary home: its own configuration, approvals, and
//! session daemon, against the `mcp-test-server` example.

use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

/// A temporary directory with every location `mcp` reads pointed inside it: `home`, `config`
/// (`XDG_CONFIG_HOME`), `state` (`XDG_STATE_HOME`), `run` (`XDG_RUNTIME_DIR`), and `project`,
/// a git worktree that commands run in.
pub(crate) struct Fixture {
    dir: TempDir,
    env: Vec<(String, String)>,
    cwd: PathBuf,
}

/// What one run of `mcp` produced.
#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) status: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Outcome {
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

impl Fixture {
    /// No configuration files, no approvals, no daemon.
    pub(crate) fn new() -> Fixture {
        let dir = tempfile::Builder::new().prefix("mcp-test-").tempdir().expect("a temp dir");
        let root = dir.path().canonicalize().expect("a canonical temp dir");
        for directory in ["home", "config", "state", "project/.git"] {
            std::fs::create_dir_all(root.join(directory)).expect("a fixture directory");
        }
        std::fs::DirBuilder::new().mode(0o700).create(root.join("run")).expect("a runtime dir");
        let path = |name: &str| root.join(name).display().to_string();
        Fixture {
            env: vec![
                ("HOME".into(), path("home")),
                ("XDG_CONFIG_HOME".into(), path("config")),
                ("XDG_STATE_HOME".into(), path("state")),
                ("XDG_RUNTIME_DIR".into(), path("run")),
            ],
            cwd: root.join("project"),
            dir,
        }
    }

    /// A project whose `.mcp.json` declares `demo`, the test server over stdio, which the
    /// registered examples use.
    pub(crate) fn seeded() -> Fixture {
        let fixture = Fixture::new();
        fixture.write_project(&json!({ "mcpServers": { "demo": stdio_server(&[]) } }));
        fixture
    }

    pub(crate) fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().expect("a canonical temp dir")
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.root().join(relative)
    }

    /// The project: a git worktree with the project `.mcp.json`, where commands run unless
    /// [`Fixture::run_in`] moves them.
    pub(crate) fn project(&self) -> PathBuf {
        self.root().join("project")
    }

    pub(crate) fn write_project(&self, config: &Value) {
        write_json(&self.project().join(".mcp.json"), config);
    }

    pub(crate) fn write_user(&self, config: &Value) {
        write_json(&self.path("config/.mcp.json"), config);
    }

    pub(crate) fn approvals_file(&self) -> PathBuf {
        self.path("state/mcp/approvals.json")
    }

    /// Records an approval as `mcp servers approve` would, without asking anyone.
    pub(crate) fn approve(&self, server: &str, fingerprint: &str) {
        let mut approvals = std::fs::read_to_string(self.approvals_file())
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or_else(|| json!({ "version": 1, "approvals": [] }));
        approvals["approvals"].as_array_mut().expect("an approvals list").push(json!({
            "repository": self.project().join(".git").display().to_string(),
            "file": ".mcp.json",
            "server": server,
            "fingerprint": fingerprint,
            "target": null,
            "approvedAt": "2026-11-22T00:00:00Z",
        }));
        write_json(&self.approvals_file(), &approvals);
    }

    /// Runs later commands in `directory` instead of the project.
    pub(crate) fn run_in(&mut self, directory: PathBuf) {
        self.cwd = directory;
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

    /// Starts `mcp` with piped standard streams and returns while it runs.
    pub(crate) fn start(&self, args: &[&str]) -> Child {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcp"));
        command
            .args(args)
            .current_dir(&self.cwd)
            .env_remove("MCP_REQUIRE_APPROVAL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command.spawn().expect("mcp runs")
    }

    /// Every file under the fixture except the daemon's runtime directory, to prove a command
    /// changed nothing.
    pub(crate) fn snapshot(&self) -> Vec<(PathBuf, Option<Vec<u8>>)> {
        fn walk(directory: &Path, skip: &Path, entries: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
            let mut children: Vec<PathBuf> = std::fs::read_dir(directory)
                .expect("a readable directory")
                .map(|entry| entry.expect("an entry").path())
                .collect();
            children.sort();
            for path in children {
                if path == skip {
                    continue;
                }
                if path.is_dir() {
                    entries.push((path.clone(), None));
                    walk(&path, skip, entries);
                } else {
                    entries.push((path.clone(), Some(std::fs::read(&path).expect("readable"))));
                }
            }
        }
        let mut entries = Vec::new();
        walk(&self.root(), &self.path("run"), &mut entries);
        entries
    }
}

impl Drop for Fixture {
    /// Stops this fixture's session daemon, and with it every server it started.
    fn drop(&mut self) {
        let pid_file =
            self.path("run").join("mcp").join(format!("{}.pid", env!("CARGO_PKG_VERSION")));
        let Some(pid) =
            std::fs::read_to_string(pid_file).ok().and_then(|text| text.trim().parse::<i32>().ok())
        else {
            return;
        };
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

fn write_json(path: &Path, value: &Value) {
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
    std::fs::write(path, serde_json::to_string_pretty(value).expect("JSON")).expect("writable");
}

/// The test server's executable: `cargo test` builds the package's examples into
/// `examples/` beside the `mcp` binary.
pub(crate) fn test_server() -> PathBuf {
    let profile = Path::new(env!("CARGO_BIN_EXE_mcp")).parent().expect("the profile directory");
    let server = profile.join("examples").join("mcp-test-server");
    assert!(
        server.exists(),
        "{} is missing; run the tests with cargo test -p mcp",
        server.display()
    );
    server
}

/// A stdio entry for the test server with `args`.
pub(crate) fn stdio_server(args: &[&str]) -> Value {
    json!({ "command": test_server().display().to_string(), "args": args })
}

/// The test server over Streamable HTTP, running until dropped.
pub(crate) struct HttpServer {
    child: Child,
    pub(crate) url: String,
}

impl HttpServer {
    pub(crate) fn start(args: &[&str]) -> HttpServer {
        let mut child = Command::new(test_server())
            .args(["--http", "127.0.0.1:0"])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the test server starts");
        let mut line = String::new();
        let stdout = child.stdout.take().expect("piped stdout");
        std::io::BufRead::read_line(&mut std::io::BufReader::new(stdout), &mut line)
            .expect("its URL");
        HttpServer { child, url: line.trim().to_string() }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
