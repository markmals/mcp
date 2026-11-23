//! Where `mcp` runs: the git worktree around the working directory, the project file, and the
//! workspace that sessions and approvals belong to.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

/// The file name both configuration files use.
pub const FILE_NAME: &str = ".mcp.json";

/// The git worktree enclosing a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub root: PathBuf,
    /// The repository's common git directory, which every worktree of one clone shares.
    pub common_dir: PathBuf,
}

/// Why the workspace is the directory it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceReason {
    /// It holds the project file.
    ProjectFile,
    /// It is the root of the git worktree; there is no project file.
    GitWorktree,
    /// Neither a project file nor a git worktree encloses the working directory.
    WorkingDirectory,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub reason: WorkspaceReason,
    pub worktree: Option<Worktree>,
    pub project_file: Option<PathBuf>,
}

impl Workspace {
    /// Finds the project file and the workspace for the working directory `cwd`, which must be
    /// absolute. A project file that is `user_file` is not a project file.
    pub fn locate(cwd: &Path, user_file: &Path) -> Workspace {
        let cwd = canonical(cwd);
        let worktree = enclosing_worktree(&cwd);
        let searched: Vec<&Path> = match &worktree {
            Some(worktree) => directories_through(&cwd, &worktree.root),
            None => vec![cwd.as_path()],
        };
        let user_file = canonical(user_file);
        let project_file = searched
            .into_iter()
            .map(|directory| directory.join(FILE_NAME))
            .find(|candidate| candidate.is_file())
            .filter(|candidate| canonical(candidate) != user_file);
        let (root, reason) = match (&project_file, &worktree) {
            (Some(file), _) => (
                file.parent().map_or_else(|| cwd.clone(), Path::to_path_buf),
                WorkspaceReason::ProjectFile,
            ),
            (None, Some(worktree)) => (worktree.root.clone(), WorkspaceReason::GitWorktree),
            (None, None) => (cwd.clone(), WorkspaceReason::WorkingDirectory),
        };
        Workspace { root, reason, worktree, project_file }
    }

    /// What an approval of the project file's servers is bound to: the repository and the
    /// project file's path inside the worktree; outside git, the file's directory and its name.
    pub fn approval_scope(&self) -> Option<(String, String)> {
        let file = self.project_file.as_ref()?;
        let scope = match &self.worktree {
            Some(worktree) => (
                worktree.common_dir.display().to_string(),
                file.strip_prefix(&worktree.root).unwrap_or(file).display().to_string(),
            ),
            None => (self.root.display().to_string(), FILE_NAME.to_string()),
        };
        Some(scope)
    }
}

/// The directories from `start` upward, up to and including `last`.
fn directories_through<'a>(start: &'a Path, last: &Path) -> Vec<&'a Path> {
    let mut directories = Vec::new();
    for directory in start.ancestors() {
        directories.push(directory);
        if directory == last {
            break;
        }
    }
    directories
}

/// `path` with symbolic links resolved, or as given when it does not exist.
pub(super) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The nearest directory at or above `start` holding a `.git` directory or file.
pub(super) fn enclosing_worktree(start: &Path) -> Option<Worktree> {
    start.ancestors().find_map(|directory| {
        let dot_git = directory.join(".git");
        let common_dir = if dot_git.is_dir() {
            dot_git
        } else if dot_git.is_file() {
            linked_common_dir(&dot_git)?
        } else {
            return None;
        };
        Some(Worktree { root: directory.to_path_buf(), common_dir: canonical(&common_dir) })
    })
}

/// A linked worktree's or submodule's `.git` file names its git directory; a linked worktree's
/// git directory has a `commondir` file naming the clone's shared one.
fn linked_common_dir(dot_git: &Path) -> Option<PathBuf> {
    let contents = std::fs::read_to_string(dot_git).ok()?;
    let target = contents.lines().find_map(|line| line.strip_prefix("gitdir:"))?.trim();
    let git_dir = dot_git.parent()?.join(target);
    match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(common) => Some(git_dir.join(common.trim())),
        Err(_) => Some(git_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn the_nearest_project_file_below_the_worktree_root_wins() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        std::fs::create_dir_all(root.join("repo/.git")).unwrap();
        write(&root.join(".mcp.json"), "{}");
        write(&root.join("repo/.mcp.json"), "{}");
        std::fs::create_dir_all(root.join("repo/a/b")).unwrap();
        let workspace = Workspace::locate(&root.join("repo/a/b"), &root.join("user/.mcp.json"));
        assert_eq!(workspace.project_file, Some(root.join("repo/.mcp.json")));
        assert_eq!(workspace.root, root.join("repo"));
        assert_eq!(workspace.reason, WorkspaceReason::ProjectFile);
    }

    #[test]
    fn the_search_stops_at_the_worktree_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        write(&root.join(".mcp.json"), "{}");
        std::fs::create_dir_all(root.join("repo/.git")).unwrap();
        std::fs::create_dir_all(root.join("repo/src")).unwrap();
        let workspace = Workspace::locate(&root.join("repo/src"), &root.join("user/.mcp.json"));
        assert_eq!(workspace.project_file, None);
        assert_eq!(workspace.root, root.join("repo"));
        assert_eq!(workspace.reason, WorkspaceReason::GitWorktree);
    }

    #[test]
    fn outside_git_only_the_working_directory_is_searched() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        write(&root.join(".mcp.json"), "{}");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let nested = Workspace::locate(&root.join("sub"), &root.join("user/.mcp.json"));
        assert_eq!(nested.project_file, None);
        assert_eq!(nested.reason, WorkspaceReason::WorkingDirectory);
        let here = Workspace::locate(&root, &root.join("user/.mcp.json"));
        assert_eq!(here.project_file, Some(root.join(".mcp.json")));
        assert_eq!(here.approval_scope(), Some((root.display().to_string(), FILE_NAME.into())));
    }

    #[test]
    fn the_user_file_is_never_the_project_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        std::fs::create_dir_all(root.join(".git")).unwrap();
        write(&root.join(".mcp.json"), "{}");
        let workspace = Workspace::locate(&root, &root.join(".mcp.json"));
        assert_eq!(workspace.project_file, None);
    }

    #[test]
    fn linked_worktrees_share_the_clone_s_common_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        std::fs::create_dir_all(root.join("main/.git/worktrees/feature")).unwrap();
        write(&root.join("main/.git/worktrees/feature/commondir"), "../..\n");
        write(&root.join("feature/.git"), "gitdir: ../main/.git/worktrees/feature\n");
        write(&root.join("feature/tools/.mcp.json"), "{}");
        let main = enclosing_worktree(&root.join("main")).unwrap();
        let feature = Workspace::locate(&root.join("feature/tools"), &root.join("u/.mcp.json"));
        assert_eq!(feature.worktree.as_ref().unwrap().common_dir, main.common_dir);
        assert_eq!(
            feature.approval_scope(),
            Some((main.common_dir.display().to_string(), "tools/.mcp.json".into()))
        );
    }
}
