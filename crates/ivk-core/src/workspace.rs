//! Kernel-level workspace compositions.
//!
//! These orchestrate a [`GitBackend`] plus filesystem operations. They are
//! the pieces that will grow lifecycle state (journal, locks, SQLite
//! registry) in Phase B; today they carry exactly the semantics the CLI
//! shipped with in v0.0.x.

use std::fmt;
use std::fs;
use std::path::Path;

use crate::git::GitBackend;

/// Workspace removal failed even after the filesystem fallback.
#[derive(Debug)]
pub struct RemoveWorkspaceError;

impl fmt::Display for RemoveWorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "could not remove workspace")
    }
}

impl std::error::Error for RemoveWorkspaceError {}

/// Remove a workspace directory and its worktree admin entry.
///
/// Semantics (unchanged from v0.0.x):
///   1. Ask git to remove the worktree (`--force`: dirty trees go too).
///   2. If git refuses (broken pointer, missing admin), fall back to
///      deleting the directory ourselves.
///   3. If the directory is gone either way, prune stale admin entries and
///      report success; otherwise fail.
pub fn remove_workspace(
    git: &dyn GitBackend,
    repo: &Path,
    ws_path: &Path,
) -> Result<(), RemoveWorkspaceError> {
    let cleaned = match git.remove_worktree(repo, ws_path) {
        Ok(()) => true,
        Err(_) => {
            let _ = fs::remove_dir_all(ws_path);
            !ws_path.exists()
        }
    };
    if cleaned {
        let _ = git.prune_worktrees(repo);
        Ok(())
    } else {
        Err(RemoveWorkspaceError)
    }
}
