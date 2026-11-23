//! Where the session daemon of this `mcp` version keeps its socket, and the directory's checks.

use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::failure::Failure;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The daemon's directory and the files in it, all named for this `mcp` version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonPaths {
    pub directory: PathBuf,
    pub socket: PathBuf,
    /// Holds the running daemon's process id, locked for the daemon's life.
    pub pid_file: PathBuf,
    /// Locked by a client while it starts the daemon, so only one client starts it.
    pub start_lock: PathBuf,
}

impl DaemonPaths {
    /// `$XDG_RUNTIME_DIR/mcp/` when that variable is an absolute path, else `$TMPDIR/mcp-<uid>/`
    /// (`/tmp` when `TMPDIR` is not absolute), with files named for this `mcp` version.
    pub fn resolve(env: impl Fn(&str) -> Option<String>) -> DaemonPaths {
        let absolute = |name: &str| env(name).map(PathBuf::from).filter(|path| path.is_absolute());
        let directory = match absolute("XDG_RUNTIME_DIR") {
            Some(runtime) => runtime.join("mcp"),
            None => absolute("TMPDIR")
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(format!("mcp-{}", rustix::process::getuid().as_raw())),
        };
        DaemonPaths::in_directory(directory)
    }

    /// The paths beside `socket`, as the daemon process derives them from its one argument.
    pub(crate) fn for_socket(socket: &Path) -> Option<DaemonPaths> {
        socket.parent().map(|directory| DaemonPaths::in_directory(directory.to_path_buf()))
    }

    fn in_directory(directory: PathBuf) -> DaemonPaths {
        DaemonPaths {
            socket: directory.join(format!("{VERSION}.sock")),
            pid_file: directory.join(format!("{VERSION}.pid")),
            start_lock: directory.join(format!("{VERSION}.lock")),
            directory,
        }
    }

    /// Creates the directory, `0700`, when it is missing, then checks it as
    /// [`DaemonPaths::check_directory`] does.
    pub(crate) fn ensure_directory(&self) -> Result<(), Failure> {
        if let Some(parent) = self.directory.parent() {
            fs::create_dir_all(parent).map_err(|error| self.directory_failure("create", &error))?;
        }
        match DirBuilder::new().mode(0o700).create(&self.directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(self.directory_failure("create", &error)),
        }
        self.check_directory()
    }

    /// Fails unless the directory is a real directory the current user owns, because the socket
    /// in it receives configured secrets; removes any group or other access.
    pub(crate) fn check_directory(&self) -> Result<(), Failure> {
        let metadata = fs::symlink_metadata(&self.directory)
            .map_err(|error| self.directory_failure("read", &error))?;
        let uid = rustix::process::getuid().as_raw();
        if !metadata.is_dir() || metadata.uid() != uid {
            return Err(Failure::precondition(format!(
                "the session daemon's directory {} is not a directory owned by the current user",
                self.directory.display()
            ))
            .with_details(self.details())
            .suggest(format!(
                "remove {} or set XDG_RUNTIME_DIR to a directory only you can open",
                self.directory.display()
            )));
        }
        if metadata.mode() & 0o077 != 0 {
            fs::set_permissions(&self.directory, Permissions::from_mode(0o700))
                .map_err(|error| self.directory_failure("restrict", &error))?;
        }
        Ok(())
    }

    fn directory_failure(&self, action: &str, error: &io::Error) -> Failure {
        Failure::unavailable(format!(
            "cannot {action} the session daemon's directory {}: {error}",
            self.directory.display()
        ))
        .with_details(self.details())
    }

    fn details(&self) -> serde_json::Value {
        serde_json::json!({ "directory": self.directory.display().to_string() })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn resolve(variables: &[(&str, &str)]) -> DaemonPaths {
        let variables: HashMap<String, String> =
            variables.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();
        DaemonPaths::resolve(|name| variables.get(name).cloned())
    }

    fn temporary_directory(base: &str) -> PathBuf {
        Path::new(base).join(format!("mcp-{}", rustix::process::getuid().as_raw()))
    }

    #[test]
    fn an_absolute_runtime_directory_wins() {
        let paths = resolve(&[("XDG_RUNTIME_DIR", "/run/user/501"), ("TMPDIR", "/var/tmp")]);
        assert_eq!(paths.directory, Path::new("/run/user/501/mcp"));
        assert_eq!(paths.socket, Path::new(&format!("/run/user/501/mcp/{VERSION}.sock")));
        assert_eq!(paths.pid_file, Path::new(&format!("/run/user/501/mcp/{VERSION}.pid")));
        assert_eq!(paths.start_lock, Path::new(&format!("/run/user/501/mcp/{VERSION}.lock")));
    }

    #[test]
    fn a_relative_runtime_directory_falls_back_to_the_temporary_directory() {
        let paths = resolve(&[("XDG_RUNTIME_DIR", "run/user"), ("TMPDIR", "/var/tmp")]);
        assert_eq!(paths.directory, temporary_directory("/var/tmp"));
    }

    #[test]
    fn without_a_usable_temporary_directory_the_daemon_uses_tmp() {
        assert_eq!(resolve(&[]).directory, temporary_directory("/tmp"));
        assert_eq!(resolve(&[("TMPDIR", "tmp")]).directory, temporary_directory("/tmp"));
        assert_eq!(resolve(&[("XDG_RUNTIME_DIR", "")]).directory, temporary_directory("/tmp"));
    }

    #[test]
    fn the_daemon_finds_its_files_from_its_socket() {
        let paths = resolve(&[("XDG_RUNTIME_DIR", "/run/user/501")]);
        assert_eq!(DaemonPaths::for_socket(&paths.socket), Some(paths));
    }

    #[test]
    fn a_missing_directory_is_created_private() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let runtime = root.path().join("runtime");
        let paths = DaemonPaths::in_directory(runtime.join("mcp"));
        paths.ensure_directory().map_err(|failure| failure.message)?;
        assert_eq!(fs::metadata(&paths.directory)?.mode() & 0o777, 0o700);
        Ok(())
    }

    #[test]
    fn an_open_directory_is_made_private() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let paths = DaemonPaths::in_directory(root.path().join("mcp"));
        DirBuilder::new().mode(0o755).create(&paths.directory)?;
        fs::set_permissions(&paths.directory, Permissions::from_mode(0o755))?;
        paths.ensure_directory().map_err(|failure| failure.message)?;
        assert_eq!(fs::metadata(&paths.directory)?.mode() & 0o777, 0o700);
        Ok(())
    }

    #[test]
    fn a_symlink_is_refused() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let target = root.path().join("elsewhere");
        fs::create_dir(&target)?;
        let paths = DaemonPaths::in_directory(root.path().join("mcp"));
        std::os::unix::fs::symlink(&target, &paths.directory)?;
        let failure = paths.ensure_directory().err().ok_or("a symlink was accepted")?;
        assert_eq!(failure.code, crate::failure::ErrorCode::PreconditionFailed);
        Ok(())
    }
}
