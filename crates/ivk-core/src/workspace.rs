//! Kernel-level workspace compositions.
//!
//! These orchestrate a [`GitBackend`] plus filesystem operations. They are
//! the pieces that will grow lifecycle state (journal, locks, SQLite
//! registry) in Phase B; today they carry exactly the semantics the CLI
//! shipped with in v0.0.x.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::git::GitBackend;

/// Advisory file lock serializing `git worktree add` per repository.
///
/// git's worktree admin setup races against itself when N processes add
/// worktrees to the same repo simultaneously (observed: `fatal: failed to
/// read .git/worktrees/<name>/commondir` at ~30 parallel `ivk new`
/// processes). The add itself takes milliseconds, so serializing it costs
/// nothing while materialization stays fully parallel.
///
/// Fail-open: if the lock cannot be acquired within the timeout, the caller
/// proceeds unlocked — a rare git race beats deadlocking every agent.
pub(crate) struct WorktreeAddLock {
    path: PathBuf,
}

const LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_STALE_AFTER: Duration = Duration::from_secs(60);

impl WorktreeAddLock {
    /// Lock file lives inside `.git/` next to git's own transient locks.
    pub(crate) fn acquire(repo: &Path) -> Option<Self> {
        let path = repo.join(".git").join("ivk-worktree-add.lock");
        let deadline = std::time::Instant::now() + LOCK_TIMEOUT;
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Some(Self { path }),
                Err(_) => {
                    // A holder that died leaves the file behind; age it out.
                    if let Ok(md) = fs::metadata(&path) {
                        if let Ok(age) = md.modified().and_then(|m| {
                            std::time::SystemTime::now()
                                .duration_since(m)
                                .map_err(|e| std::io::Error::other(e.to_string()))
                        }) {
                            if age >= LOCK_STALE_AFTER {
                                let _ = fs::remove_file(&path);
                                continue;
                            }
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        return None; // fail-open
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        }
    }
}

impl Drop for WorktreeAddLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

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
