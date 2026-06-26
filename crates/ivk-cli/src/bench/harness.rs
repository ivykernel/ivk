//! Shared scaffolding for `ivk bench *`:
//!   - [`Prelude`]    — repo sanity, ts/pid identity, df-free baseline, env block
//!   - [`BenchDir`]   — RAII guard that wipes the bench dir on Drop (even on panic)
//!   - [`Stats`]      — p50/p90/p99 over a Vec<u128> of microseconds
//!   - `df_free_kb`   — wraps `df -k <path>` to read the volume's free space
//!   - `dir_apparent_bytes` / `dir_allocated_bytes` — disk-triad components
//!   - `human_bytes` / `human_ms` — small string helpers reused across envelopes
//!
//! Design contract: every primary bench subcommand acquires a `BenchDir` early
//! and holds it for the entire run, so any panic — including SIGINT inside a
//! materialize loop — still cleans up the temp tree on unwind.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde::Serialize;

/// Per-run identity + baseline measurements. Cheap to clone references out of.
pub(crate) struct Prelude {
    pub cwd: PathBuf,
    pub bench_root: PathBuf,
    pub from_sha: String,
    pub df_free_before_kb: u64,
    pub env: EnvBlock,
}

#[derive(Serialize, Clone)]
pub(crate) struct EnvBlock {
    pub os: &'static str,
    pub strategy: &'static str,
    pub ivk_version: &'static str,
    pub git_version: String,
}

/// Caller passes the command name (used in the bench-dir prefix) and the
/// CLI-supplied prefix (currently always `"b"`). The unique directory ends up at
/// `.ivk/bench/<prefix>-<command>-<pid>-<ts_nanos>/`.
pub(crate) fn prepare(command: &str, prefix: &str) -> Result<Prelude, PreludeError> {
    let cwd = std::env::current_dir().map_err(|e| PreludeError {
        code: "io_error",
        message: format!("cannot resolve cwd: {}", e),
        next_command: "ivk doctor".into(),
    })?;
    if !cwd.join(".git").exists() {
        return Err(PreludeError {
            code: "not_a_git_repo",
            message: format!("no .git at {}", cwd.display()),
            next_command: "git init".into(),
        });
    }
    // Resolve HEAD as a commit. Used both for refusing on no-commit repos and
    // for stamping `from_sha` into the envelope.
    let head = match Command::new("git")
        .arg("-C")
        .arg(&cwd)
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            return Err(PreludeError {
                code: "no_commits",
                message: "HEAD does not point at a commit (fresh repo?)".into(),
                next_command: "git commit --allow-empty -m bootstrap".into(),
            });
        }
    };

    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let bench_root = cwd
        .join(".ivk")
        .join("bench")
        .join(format!("{}-{}-{}-{}", prefix, command, pid, ts));
    if let Err(e) = fs::create_dir_all(&bench_root) {
        return Err(PreludeError {
            code: "io_error",
            message: format!("cannot create {}: {}", bench_root.display(), e),
            next_command: "ivk doctor".into(),
        });
    }

    let df_free_before_kb = df_free_kb(&cwd).unwrap_or(0);

    Ok(Prelude {
        cwd,
        bench_root,
        from_sha: head,
        df_free_before_kb,
        env: env_block(),
    })
}

pub(crate) struct PreludeError {
    pub code: &'static str,
    pub message: String,
    pub next_command: String,
}

pub(crate) fn env_block() -> EnvBlock {
    let git = Command::new("git")
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    EnvBlock {
        os: env_os(),
        strategy: env_strategy(),
        ivk_version: env!("CARGO_PKG_VERSION"),
        git_version: git,
    }
}

#[cfg(target_os = "macos")]
const fn env_os() -> &'static str {
    "macos"
}
#[cfg(target_os = "linux")]
const fn env_os() -> &'static str {
    "linux"
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const fn env_os() -> &'static str {
    "other"
}

#[cfg(target_os = "macos")]
const fn env_strategy() -> &'static str {
    "apfs-clonefile"
}
#[cfg(target_os = "linux")]
const fn env_strategy() -> &'static str {
    "linux-reflink-via-cp"
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const fn env_strategy() -> &'static str {
    "unsupported"
}

/// RAII guard: on Drop, removes the bench root and prunes git worktree admin.
/// Runs even on panic so a failed bench never leaves stray workspaces behind.
pub(crate) struct BenchDir {
    pub path: PathBuf,
    pub parent_repo: PathBuf,
}

impl BenchDir {
    pub(crate) fn new(path: PathBuf, parent_repo: PathBuf) -> Self {
        Self { path, parent_repo }
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        if !self.path.exists() {
            return;
        }
        // git worktree remove for each subdir that has a `.git` pointer file.
        if let Ok(entries) = fs::read_dir(&self.path) {
            for e in entries.flatten() {
                let p = e.path();
                if p.join(".git").is_file() {
                    let _ = Command::new("git")
                        .arg("-C")
                        .arg(&self.parent_repo)
                        .args(["worktree", "remove", "--force"])
                        .arg(&p)
                        .output();
                }
            }
        }
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.parent_repo)
            .args(["worktree", "prune"])
            .output();
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Percentile summary over a list of microsecond durations.
#[derive(Serialize)]
pub(crate) struct Stats {
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
}

impl Stats {
    pub(crate) fn from_micros(mut xs: Vec<u128>) -> Self {
        if xs.is_empty() {
            return Self {
                p50_ms: 0.0,
                p90_ms: 0.0,
                p99_ms: 0.0,
                min_ms: 0.0,
                max_ms: 0.0,
                mean_ms: 0.0,
            };
        }
        xs.sort_unstable();
        let n = xs.len();
        let pick = |frac: f64| {
            let idx = ((n as f64 - 1.0) * frac).round() as usize;
            xs[idx] as f64 / 1000.0
        };
        let sum: u128 = xs.iter().sum();
        Self {
            p50_ms: pick(0.50),
            p90_ms: pick(0.90),
            p99_ms: pick(0.99),
            min_ms: *xs.first().unwrap() as f64 / 1000.0,
            max_ms: *xs.last().unwrap() as f64 / 1000.0,
            mean_ms: (sum as f64 / n as f64) / 1000.0,
        }
    }
}

/// Volume free space in KB via `df -k <path>`. Returns None on parse failure or
/// when `df` is unavailable.
pub(crate) fn df_free_kb(path: &Path) -> Option<u64> {
    let out = Command::new("df").arg("-k").arg(path).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // df -k output: a header line, then one data line. Columns vary across
    // BSD (macOS) and GNU (Linux) but field 4 (1-indexed) is "Available" on
    // both for the typical single-FS case.
    let line = stdout.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    // macOS: Filesystem 512-blocks Used Available Capacity iused ifree %iused MountedOn
    //        with -k:    1024-blocks Used Available ...
    // Linux: Filesystem 1K-blocks  Used Available Use% Mounted
    // Either way, "Available" is at index 3.
    fields.get(3)?.parse::<u64>().ok()
}

/// Sum of file logical sizes under `root` (`du -A`-equivalent). Symlinks are
/// counted as their link-target size: we follow `metadata()`. Errors are
/// silently skipped.
pub(crate) fn dir_apparent_bytes(root: &Path) -> u64 {
    if !root.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let md = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if md.file_type().is_symlink() {
                continue;
            }
            if md.is_dir() {
                stack.push(path);
            } else {
                total = total.saturating_add(md.len());
            }
        }
    }
    total
}

/// Sum of per-file *allocated* blocks under `root` (`du -k`-equivalent on Unix).
/// On APFS clonefile / Linux reflinks, this UNDER-reports because shared
/// extents are credited per-file. Reported alongside the other two for honesty.
#[cfg(unix)]
pub(crate) fn dir_allocated_bytes(root: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    if !root.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let md = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if md.file_type().is_symlink() {
                continue;
            }
            if md.is_dir() {
                stack.push(path);
            } else {
                total = total.saturating_add(md.blocks() * 512);
            }
        }
    }
    total
}

#[cfg(not(unix))]
pub(crate) fn dir_allocated_bytes(root: &Path) -> u64 {
    dir_apparent_bytes(root)
}

pub(crate) fn human_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let n = n as f64;
    if n >= GB {
        format!("{:.1} GiB", n / GB)
    } else if n >= MB {
        format!("{:.1} MiB", n / MB)
    } else if n >= KB {
        format!("{:.1} KiB", n / KB)
    } else {
        format!("{} B", n as u64)
    }
}

pub(crate) fn human_ms(ms: f64) -> String {
    if ms >= 60_000.0 {
        format!("{:.1} min", ms / 60_000.0)
    } else if ms >= 1_000.0 {
        format!("{:.1} s", ms / 1_000.0)
    } else {
        format!("{:.0} ms", ms)
    }
}
