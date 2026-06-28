//! Ivy Kernel core: workspace materialization primitives.
//!
//! This crate exposes the OS-level operation of cloning a working tree from a
//! source git repository to a destination path, plus the git plumbing needed
//! to make that destination a valid git worktree.
//!
//! It is deliberately small. Higher-level concerns (CLI, registry, lifecycle
//! state, JSON output, agent-readability) live in `ivk-cli` and future crates.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
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
}

#[derive(Debug, Clone)]
pub struct MaterializeReport {
    pub strategy: &'static str,
    pub cloned_entries: usize,
    pub skipped_entries: usize,
    pub git_worktree_add: Option<Duration>,
    pub clone_tree: Duration,
    pub git_read_tree: Option<Duration>,
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

#[cfg(target_os = "macos")]
mod backend {
    use std::ffi::CString;
    use std::io;

    extern "C" {
        // int clonefile(const char *src, const char *dst, uint32_t flags);
        fn clonefile(src: *const i8, dst: *const i8, flags: u32) -> i32;
    }

    pub fn clone_path(src: &CString, dst: &CString) -> io::Result<()> {
        let ret = unsafe { clonefile(src.as_ptr(), dst.as_ptr(), 0) };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn strategy() -> &'static str {
        "apfs-clonefile"
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use std::ffi::CString;
    use std::fs;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink as symlink_unix;
    use std::os::unix::io::AsRawFd;
    use std::path::Path;

    /// FICLONE ioctl. Linux's per-file reflink: shares all extents of `src`
    /// with `dst` until either is written to. Equivalent to APFS's
    /// `clonefile(2)` for a single file, but NOT recursive on directories;
    /// we walk the tree ourselves below.
    ///
    /// Value: `_IOW('X', 9, int)` = `0x40049409` on most architectures.
    /// Defined in `<linux/fs.h>`; not currently in the `libc` crate's
    /// public surface, so we hard-code the constant.
    const FICLONE: libc::c_ulong = 0x4004_9409;

    pub fn clone_path(src_c: &CString, dst_c: &CString) -> io::Result<()> {
        let src = Path::new(std::ffi::OsStr::from_bytes(src_c.as_bytes()));
        let dst = Path::new(std::ffi::OsStr::from_bytes(dst_c.as_bytes()));
        clone_tree(src, dst)
    }

    fn clone_tree(src: &Path, dst: &Path) -> io::Result<()> {
        let meta = fs::symlink_metadata(src)?;
        let ft = meta.file_type();
        if ft.is_file() {
            clone_file(src, dst).map_err(map_unsupported_fs)
        } else if ft.is_dir() {
            fs::create_dir(dst)?;
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                let name = entry.file_name();
                clone_tree(&src.join(&name), &dst.join(&name))?;
            }
            Ok(())
        } else if ft.is_symlink() {
            let target = fs::read_link(src)?;
            symlink_unix(target, dst)
        } else {
            // FIFOs, sockets, block/char devices: not part of a working tree
            // in any practical sense; skip silently.
            Ok(())
        }
    }

    fn clone_file(src: &Path, dst: &Path) -> io::Result<()> {
        let src_f = fs::File::open(src)?;
        let dst_f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dst)?;
        let ret = unsafe { libc::ioctl(dst_f.as_raw_fd(), FICLONE, src_f.as_raw_fd()) };
        if ret == 0 {
            Ok(())
        } else {
            // Clean up the empty file we just created so the caller doesn't
            // see a half-cloned state.
            let err = io::Error::last_os_error();
            let _ = fs::remove_file(dst);
            Err(err)
        }
    }

    /// Map `EOPNOTSUPP` / `EINVAL` from FICLONE into a human-actionable
    /// message. These are the errors users hit on ext4 or other
    /// non-reflink filesystems.
    fn map_unsupported_fs(e: io::Error) -> io::Error {
        match e.raw_os_error() {
            Some(libc::EOPNOTSUPP) | Some(libc::EINVAL) | Some(libc::ENOTTY) => {
                io::Error::other(format!(
                    "{} — the destination filesystem does not support reflink. \
                     ivk works on macOS APFS and on Linux btrfs / xfs (reflink=1) / \
                     zfs ≥ 2.2 / bcachefs. ext4 is unsupported; \
                     see docs/portability.md for the planned overlayfs fallback.",
                    e
                ))
            }
            _ => e,
        }
    }

    pub fn strategy() -> &'static str {
        "linux-ficlone"
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("ivk-core currently supports only macOS and Linux");

fn to_cstring(p: &Path) -> Result<CString, Error> {
    CString::new(p.as_os_str().as_bytes()).map_err(|_| {
        Error::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a NUL byte",
        ))
    })
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<(), Error> {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .map_err(|e| Error::Git(format!("could not launch git: {e}")))?;
    if !status.success() {
        return Err(Error::Git(format!("git {:?} exited with {status}", args)));
    }
    Ok(())
}

/// Materialize a new workspace at `opts.dst` from the git repo at `opts.src`.
///
/// Steps when `with_git == true`:
///   1. `git worktree add --no-checkout --detach <dst> HEAD` from inside `<src>`.
///   2. For each non-`.git` top-level entry in `<src>`, call `clonefile(2)` /
///      `cp --reflink=always` to clone it into `<dst>`. clonefile is recursive
///      on directories, so one syscall per top-level entry covers the subtree.
///   3. `git read-tree HEAD` inside `<dst>` to populate the index so
///      `git status` sees a clean working tree.
///
/// Steps when `with_git == false`: just step 2, with `<dst>` created as an
/// empty directory first. The workspace is not a git repo, just files.
pub fn materialize_workspace(opts: &MaterializeOptions) -> Result<MaterializeReport, Error> {
    let total_t0 = Instant::now();

    if !opts.src.join(".git").exists() {
        return Err(Error::SrcMissing(opts.src.clone()));
    }
    if opts.dst.exists() {
        return Err(Error::DstExists(opts.dst.clone()));
    }

    let mut report = MaterializeReport {
        strategy: backend::strategy(),
        cloned_entries: 0,
        skipped_entries: 0,
        git_worktree_add: None,
        clone_tree: Duration::ZERO,
        git_read_tree: None,
        total: Duration::ZERO,
    };

    if opts.with_git {
        let t0 = Instant::now();
        run_git(
            &opts.src,
            &[
                "worktree",
                "add",
                "-q",
                "--no-checkout",
                "--detach",
                opts.dst.to_str().ok_or_else(|| {
                    Error::Io(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "dst is not valid UTF-8",
                    ))
                })?,
                "HEAD",
            ],
        )?;
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
        let src_c = to_cstring(&src_path)?;
        let dst_c = to_cstring(&dst_path)?;
        backend::clone_path(&src_c, &dst_c).map_err(|e| Error::Clone {
            src: src_path.clone(),
            dst: dst_path.clone(),
            source: e,
        })?;
        report.cloned_entries += 1;
    }
    report.clone_tree = t0.elapsed();

    if opts.with_git {
        let t0 = Instant::now();
        run_git(&opts.dst, &["read-tree", "HEAD"])?;
        report.git_read_tree = Some(t0.elapsed());
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
