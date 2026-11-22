//! `approvals.json`: the approvals a person has given, kept in `mcp`'s state directory.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::failure::Failure;

/// How long a write waits for another `mcp` process to release the file before it reports
/// the store busy.
const LOCK_WAIT: Duration = Duration::from_secs(5);

/// What one approval is bound to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    /// The git common directory, or the project file's directory outside git.
    pub repository: String,
    /// The project file's path relative to the worktree root.
    pub file: String,
    pub server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    #[serde(flatten)]
    pub binding: Binding,
    pub fingerprint: String,
    /// The command line or URL as written, for a person reading the file.
    pub target: Option<String>,
    pub approved_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Approvals {
    pub version: u32,
    pub approvals: Vec<Approval>,
}

impl Approvals {
    /// Every fingerprint approved for `binding`.
    pub fn fingerprints<'a>(&'a self, binding: &'a Binding) -> impl Iterator<Item = &'a str> {
        self.approvals
            .iter()
            .filter(move |approval| &approval.binding == binding)
            .map(|approval| approval.fingerprint.as_str())
    }
}

pub struct ApprovalStore {
    pub path: PathBuf,
}

impl ApprovalStore {
    /// `$XDG_STATE_HOME/mcp/approvals.json` when that variable is an absolute path, else
    /// `~/.local/state/mcp/approvals.json`.
    ///
    /// # Errors
    ///
    /// `PRECONDITION_FAILED` when neither variable locates it.
    pub fn locate(env: &impl Fn(&str) -> Option<String>) -> Result<ApprovalStore, Failure> {
        let absolute = |key: &str| env(key).map(PathBuf::from).filter(|path| path.is_absolute());
        absolute("XDG_STATE_HOME")
            .or_else(|| absolute("HOME").map(|home| home.join(".local/state")))
            .map(|state| ApprovalStore { path: state.join("mcp").join("approvals.json") })
            .ok_or_else(|| {
                Failure::precondition(
                    "cannot locate the approvals file: neither XDG_STATE_HOME nor HOME is an \
                     absolute path",
                )
            })
    }

    /// Reads every approval; an absent file holds none.
    ///
    /// # Errors
    ///
    /// `PRECONDITION_FAILED` when the file cannot be read or is not an approvals file.
    pub fn load(&self) -> Result<Approvals, Failure> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|error| self.corrupt(&error.to_string()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Approvals::default()),
            Err(error) => Err(self.corrupt(&error.to_string())),
        }
    }

    /// Applies `change` to the approvals under an exclusive lock and replaces the file
    /// atomically, mode `0600`.
    ///
    /// # Errors
    ///
    /// `UNAVAILABLE` while another process holds the lock, and `PRECONDITION_FAILED` when the
    /// file or its directory cannot be read or written.
    pub fn update<T>(&self, change: impl FnOnce(&mut Approvals) -> T) -> Result<T, Failure> {
        let directory = self.path.parent().unwrap_or(Path::new("/"));
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .map_err(|error| self.unwritable(&error))?;
        let _lock = self.lock(directory)?;
        let mut approvals = self.load()?;
        let outcome = change(&mut approvals);
        approvals.version = 1;
        let text = serde_json::to_string_pretty(&approvals)
            .map_err(|error| Failure::internal(format!("cannot serialize approvals: {error}")))?;
        let temporary = self.path.with_extension("json.tmp");
        write_private(&temporary, &text).map_err(|error| self.unwritable(&error))?;
        std::fs::rename(&temporary, &self.path).map_err(|error| self.unwritable(&error))?;
        Ok(outcome)
    }

    fn lock(&self, directory: &Path) -> Result<File, Failure> {
        let path = directory.join("approvals.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| self.unwritable(&error))?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < LOCK_WAIT => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(Failure::unavailable(format!(
                        "another mcp process is changing {}",
                        self.path.display()
                    ))
                    .with_details(json!({ "file": self.path.display().to_string() })));
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(self.unwritable(&error)),
            }
        }
    }

    fn corrupt(&self, problem: &str) -> Failure {
        Failure::precondition(format!(
            "cannot read the approvals in {}: {problem}",
            self.path.display()
        ))
        .with_details(json!({ "file": self.path.display().to_string() }))
        .suggest("Move the file aside; every project server then needs a person's approval again")
    }

    fn unwritable(&self, error: &std::io::Error) -> Failure {
        Failure::precondition(format!("cannot write {}: {error}", self.path.display()))
            .with_details(json!({ "file": self.path.display().to_string() }))
    }
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut file =
        OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(server: &str) -> Binding {
        Binding { repository: "/repo/.git".into(), file: ".mcp.json".into(), server: server.into() }
    }

    #[test]
    fn records_approvals_in_a_private_file_and_reads_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = ApprovalStore { path: dir.path().join("mcp/approvals.json") };
        assert!(store.load().unwrap().approvals.is_empty());
        store
            .update(|approvals| {
                approvals.approvals.push(Approval {
                    binding: binding("a"),
                    fingerprint: "sha256:1".into(),
                    target: Some("npx a".into()),
                    approved_at: "2026-11-22T00:00:00Z".into(),
                });
            })
            .unwrap();
        let approvals = store.load().unwrap();
        assert_eq!(approvals.fingerprints(&binding("a")).collect::<Vec<_>>(), ["sha256:1"]);
        assert_eq!(approvals.fingerprints(&binding("b")).count(), 0);
        let mode = std::fs::metadata(&store.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_corrupt_file_is_a_precondition_failure() {
        let dir = tempfile::tempdir().unwrap();
        let store = ApprovalStore { path: dir.path().join("approvals.json") };
        std::fs::write(&store.path, "not json").unwrap();
        assert_eq!(store.load().unwrap_err().code, crate::failure::ErrorCode::PreconditionFailed);
    }

    #[test]
    fn the_state_directory_follows_xdg() {
        let env = |key: &str| (key == "XDG_STATE_HOME").then(|| "/state".to_string());
        assert_eq!(
            ApprovalStore::locate(&env).unwrap().path,
            PathBuf::from("/state/mcp/approvals.json")
        );
        let home = |key: &str| (key == "HOME").then(|| "/home/me".to_string());
        assert_eq!(
            ApprovalStore::locate(&home).unwrap().path,
            PathBuf::from("/home/me/.local/state/mcp/approvals.json")
        );
    }
}
