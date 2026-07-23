//! Ivy Kernel core: the workspace kernel.
//!
//! This crate owns every git and filesystem operation behind two seams
//! (see `ivk_workspace_kernel_plan_v3.md`):
//!
//!   - [`GitBackend`] — git operations. Desktop uses [`GitCliBackend`]
//!     (shells out to the real `git`, the compatibility baseline); a
//!     libgit2 backend for platforms that cannot spawn processes (iOS)
//!     arrives behind a feature flag in Phase C.
//!   - [`Materializer`] — working-tree cloning. [`CowMaterializer`] is the
//!     copy-on-write default (APFS `clonefile(2)` / Linux `FICLONE`);
//!     [`CopyMaterializer`] is the explicit plain-copy fallback.
//!
//! Higher-level concerns (CLI parsing, JSON envelopes, agent-readability)
//! live in `ivk-cli` and future frontends (`ivk-ffi`).

pub mod git;
pub mod materializer;
pub mod registry;
pub mod workspace;

pub use git::cli::GitCliBackend;
pub use git::{
    CommitIdentity, DiffStat, DiffTarget, GitBackend, GitError, MergeCheck, RefEntry, StatusEntry,
    StatusSummary,
};
pub use materializer::{default_strategy, CopyMaterializer, CowMaterializer, Materializer};
pub use registry::{
    BeginCreate, ChangesetRecord, PendingOp, Registry, RegistryError, SyncReport, WorkspaceRecord,
    WorkspaceState,
};
pub use workspace::{remove_workspace, RemoveWorkspaceError};

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct MaterializeOptions {
    /// The source git repository (must be initialized; HEAD must resolve).
    pub src: PathBuf,
    /// Where to create the workspace. Must not exist.
    pub dst: PathBuf,
    /// If true, also set up `.git` worktree admin + populate the index from HEAD.
    /// If false, the workspace is a plain directory of files with no git affordance.
    pub with_git: bool,
    /// Base the workspace on this revision instead of HEAD. Requires
    /// `with_git`. The working tree is CoW-cloned from the source as usual,
    /// then only the paths that differ between HEAD and `rev` are rewritten
    /// (ignored files — caches, build artifacts — survive the fixup).
    pub rev: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MaterializeReport {
    pub strategy: &'static str,
    pub cloned_entries: usize,
    pub skipped_entries: usize,
    pub git_worktree_add: Option<Duration>,
    pub clone_tree: Duration,
    pub git_read_tree: Option<Duration>,
    /// Time spent aligning the cloned tree to a non-HEAD `rev` (restore +
    /// clean). `None` when the workspace is based on HEAD.
    pub git_fixup: Option<Duration>,
    pub total: Duration,
}

#[derive(Debug)]
pub enum Error {
    SrcMissing(PathBuf),
    DstExists(PathBuf),
    Git(String),
    Clone {
        src: PathBuf,
        dst: PathBuf,
        source: io::Error,
    },
    Io(io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::SrcMissing(p) => write!(f, "source repo not found: {}", p.display()),
            Error::DstExists(p) => write!(f, "destination already exists: {}", p.display()),
            Error::Git(m) => write!(f, "git command failed: {m}"),
            Error::Clone { src, dst, source } => write!(
                f,
                "clone failed for {} -> {}: {source}",
                src.display(),
                dst.display()
            ),
            Error::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Clone { source, .. } => Some(source),
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<GitError> for Error {
    fn from(e: GitError) -> Self {
        Error::Git(e.to_string())
    }
}

/// Materialize a new workspace at `opts.dst` from the git repo at `opts.src`,
/// using the platform-default backends ([`GitCliBackend`] +
/// [`CowMaterializer`]).
pub fn materialize_workspace(opts: &MaterializeOptions) -> Result<MaterializeReport, Error> {
    materialize_workspace_with(&GitCliBackend::new(), &CowMaterializer, opts)
}

/// Materialize a new workspace with explicit backends.
///
/// Steps when `opts.with_git == true`:
///   1. Register a worktree at `dst` on a detached HEAD, without checkout.
///   2. For each non-`.git`/non-`.ivk` top-level entry in `src`, clone it
///      into `dst` via the materializer.
///   3. Populate `dst`'s index from HEAD so `git status` sees a clean tree.
///   4. When `opts.rev` resolves to something other than the source HEAD:
///      restore tracked files to `rev` and drop files untracked at `rev`
///      (keeping ignored ones), so only the differing paths cost real disk.
///
/// Steps when `opts.with_git == false`: just step 2, with `dst` created as an
/// empty directory first. The workspace is not a git repo, just files.
pub fn materialize_workspace_with(
    gitb: &dyn GitBackend,
    materializer: &dyn Materializer,
    opts: &MaterializeOptions,
) -> Result<MaterializeReport, Error> {
    let total_t0 = Instant::now();

    if !opts.src.join(".git").exists() {
        return Err(Error::SrcMissing(opts.src.clone()));
    }
    if opts.dst.exists() {
        return Err(Error::DstExists(opts.dst.clone()));
    }
    if opts.rev.is_some() && !opts.with_git {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "MaterializeOptions.rev requires with_git",
        )));
    }

    let mut report = MaterializeReport {
        strategy: materializer.strategy(),
        cloned_entries: 0,
        skipped_entries: 0,
        git_worktree_add: None,
        clone_tree: Duration::ZERO,
        git_read_tree: None,
        git_fixup: None,
        total: Duration::ZERO,
    };

    // Resolve a non-HEAD base up front (fails fast on a bad rev) and decide
    // whether the cloned tree needs a fixup pass. The default HEAD path pays
    // no extra git calls.
    let mut fixup_needed = false;
    let mut target = String::from("HEAD");
    if let Some(rev) = opts.rev.as_deref() {
        let target_sha = gitb.resolve_revision(&opts.src, rev)?;
        let head_sha = gitb.resolve_revision(&opts.src, "HEAD")?;
        fixup_needed = target_sha != head_sha;
        target = target_sha;
    }

    if opts.with_git {
        let t0 = Instant::now();
        // Serialize the add per repo: git's worktree admin setup races
        // against concurrent adds (see workspace::WorktreeAddLock). The
        // expensive part — materialization — stays parallel.
        let lock = workspace::WorktreeAddLock::acquire(&opts.src);
        let added = gitb.add_worktree(&opts.src, &opts.dst, &target);
        drop(lock);
        added?;
        report.git_worktree_add = Some(t0.elapsed());
    } else {
        fs::create_dir_all(&opts.dst)?;
    }

    let t0 = Instant::now();
    for entry in fs::read_dir(&opts.src)? {
        let entry = entry?;
        let name = entry.file_name();
        // Always skip these directories at the top level:
        //   .git    — shared via the worktree pointer file, not cloned
        //   .ivk    — ivk's own state lives here, including the workspaces/
        //             directory that contains this very destination. Cloning
        //             it would recurse into the dst we're building.
        if name == ".git" || name == ".ivk" {
            report.skipped_entries += 1;
            continue;
        }
        let src_path = entry.path();
        let dst_path = opts.dst.join(&name);
        materializer
            .clone_path(&src_path, &dst_path)
            .map_err(|e| Error::Clone {
                src: src_path.clone(),
                dst: dst_path.clone(),
                source: e,
            })?;
        report.cloned_entries += 1;
    }
    report.clone_tree = t0.elapsed();

    if opts.with_git {
        let t0 = Instant::now();
        gitb.populate_index(&opts.dst, "HEAD")?;
        report.git_read_tree = Some(t0.elapsed());
    }

    if fixup_needed {
        // The cloned files reflect the source working tree (≈ source HEAD);
        // the index is already at `rev`. Rewrite only what differs, then
        // drop files that don't exist at `rev` — but keep ignored paths so
        // shared caches (node_modules, target/) survive.
        let t0 = Instant::now();
        gitb.restore_worktree(&opts.dst)?;
        gitb.clean_untracked(&opts.dst, true)?;
        report.git_fixup = Some(t0.elapsed());
    }

    report.total = total_t0.elapsed();
    Ok(report)
}

pub fn absolutize(p: &Path) -> io::Result<PathBuf> {
    if p.is_absolute() {
        Ok(p.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(p))
    }
}
