//! Git backend abstraction.
//!
//! `GitBackend` is the seam that lets the kernel run on platforms where no
//! `git` binary exists (iOS cannot spawn processes). Desktop uses
//! [`cli::GitCliBackend`] — the real `git` binary stays the compatibility
//! baseline. A libgit2-based backend arrives behind a feature flag in Phase C
//! (see `ivk_workspace_kernel_plan_v3.md`); both backends must pass the same
//! parity test suite.
//!
//! The trait carries *git-level* primitives only. Kernel-level compositions
//! (create/remove a whole workspace = worktree admin + materialization +
//! lifecycle state) live in the crate root and `crate::workspace`, so a git
//! backend never needs to know about clonefile.

pub mod cli;

use std::fmt;
use std::path::Path;

/// Error from a git backend operation.
///
/// `op` is a stable, backend-independent operation label (`"rev-parse"`,
/// `"add"`, `"commit"`, `"worktree-add"`, ...) so callers can map failures to
/// their own error codes without parsing messages.
#[derive(Debug)]
pub struct GitError {
    pub op: &'static str,
    pub message: String,
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "git {} failed: {}", self.op, self.message)
    }
}

impl std::error::Error for GitError {}

/// One entry of `git status --porcelain`.
#[derive(Debug, Clone)]
pub struct StatusEntry {
    /// The two-character `XY` code (e.g. `"??"`, `" M"`).
    pub code: String,
    /// The path exactly as porcelain prints it (renames keep the full
    /// `"old -> new"` form).
    pub path: String,
}

/// Parsed working-tree status.
#[derive(Debug, Clone, Default)]
pub struct StatusSummary {
    pub entries: Vec<StatusEntry>,
}

impl StatusSummary {
    pub fn is_dirty(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn touched_paths(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.path.clone()).collect()
    }
}

/// Aggregate diff numbers. Binary files count toward `files_changed` with
/// zero insertions/deletions.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiffStat {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

/// What to diff.
#[derive(Debug, Clone, Copy)]
pub enum DiffTarget<'a> {
    /// Working tree (including index) against HEAD. Run inside a worktree.
    WorktreeToHead,
    /// `base..head` commit range. Run anywhere in the repo.
    CommitRange { base: &'a str, head: &'a str },
}

/// A ref name (short form, e.g. `agent/fix-login`) and the sha it points at.
#[derive(Debug, Clone)]
pub struct RefEntry {
    pub name: String,
    pub sha: String,
}

/// Identity used for kernel-created commits when none is configured.
#[derive(Debug, Clone)]
pub struct CommitIdentity {
    pub name: String,
    pub email: String,
}

impl CommitIdentity {
    /// The ivk bot identity — makes commits succeed in environments where
    /// global git config is absent (CI, fresh agent sandboxes).
    pub fn ivk_default() -> Self {
        Self {
            name: "ivk".into(),
            email: "ivk@ivykernel.dev".into(),
        }
    }
}

/// Git operations the kernel needs, implementable without a `git` binary.
///
/// Contract notes:
///   - `stage_all_and_commit` is the **only** committing operation, and it is
///     always an explicit call — the kernel never commits implicitly.
///   - There is deliberately no `push`: ivk exports branches/patches and
///     leaves pushing to the user (a policy-gated push may arrive in Phase E).
///   - Network operations (clone/fetch) join the trait in Phase C together
///     with the credential-callback design.
pub trait GitBackend {
    /// Stable backend identifier (`"git-cli"`, `"libgit2"`).
    fn name(&self) -> &'static str;

    /// Resolve a revision (e.g. `"HEAD"`) to a full sha.
    fn resolve_revision(&self, repo: &Path, rev: &str) -> Result<String, GitError>;

    /// Resolve a revision to git's short, unambiguous sha form.
    fn resolve_revision_short(&self, repo: &Path, rev: &str) -> Result<String, GitError>;

    /// Working-tree status of `worktree`.
    fn status(&self, worktree: &Path) -> Result<StatusSummary, GitError>;

    /// Aggregate diff numbers for `target`.
    fn diff_stat(&self, repo: &Path, target: DiffTarget) -> Result<DiffStat, GitError>;

    /// Unified diff for `target`. `binary` includes binary deltas
    /// (`git apply`-able across repos).
    fn diff_patch(
        &self,
        repo: &Path,
        target: DiffTarget,
        binary: bool,
    ) -> Result<Vec<u8>, GitError>;

    /// Paths changed by `target` (one path per entry, rename shown as the
    /// new path).
    fn changed_paths(&self, repo: &Path, target: DiffTarget) -> Result<Vec<String>, GitError>;

    /// Restore every tracked file in `worktree` to its index state
    /// (recreates deleted files, reverts modifications).
    fn restore_worktree(&self, worktree: &Path) -> Result<(), GitError>;

    /// Remove untracked files/directories from `worktree`. With
    /// `keep_ignored`, ignored paths (caches, build artifacts) survive.
    fn clean_untracked(&self, worktree: &Path, keep_ignored: bool) -> Result<(), GitError>;

    /// Register a new worktree at `dst` on a detached HEAD at `rev`,
    /// without checking out files (the materializer populates them).
    fn add_worktree(&self, repo: &Path, dst: &Path, rev: &str) -> Result<(), GitError>;

    /// Populate the index of `worktree` from `rev` so status reads clean
    /// after materialization.
    fn populate_index(&self, worktree: &Path, rev: &str) -> Result<(), GitError>;

    /// Remove a worktree and its admin entry (force: dirty trees go too).
    fn remove_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), GitError>;

    /// Prune stale worktree admin entries; returns the pruned entry names.
    fn prune_worktrees(&self, repo: &Path) -> Result<Vec<String>, GitError>;

    /// Stage everything and commit. Returns the new commit sha.
    fn stage_all_and_commit(
        &self,
        worktree: &Path,
        message: &str,
        identity: &CommitIdentity,
    ) -> Result<String, GitError>;

    /// Create (or with `force`, move) branch `branch` at `sha`.
    fn create_branch(
        &self,
        repo: &Path,
        branch: &str,
        sha: &str,
        force: bool,
    ) -> Result<(), GitError>;

    /// List refs under `prefix` (e.g. `"refs/heads/agent/"`) as short names.
    fn list_refs(&self, repo: &Path, prefix: &str) -> Result<Vec<RefEntry>, GitError>;
}
