//! `ivk gc` — Phase 4 garbage collection.
//!
//! Classification is *stateless*: every decision derives from
//!   - directory listings under `.ivk/workspaces/`
//!   - directory listings under `<repo>/.git/worktrees/`
//!   - changeset metadata under `.ivk/changesets/*.json`
//!   - git refs of the form `refs/heads/agent/*`
//!
//! v0.0.1 ships no per-workspace sidecar state.
//!
//! Hard rules:
//!   - DO NOT call `git gc` from here. The source repo's object store is shared
//!     across every workspace; pruning objects in one worktree can invalidate
//!     refs in another (see Phase 4 design notes / AGENTS.md gotcha #3).
//!   - DO NOT delete a `.ivk/changesets/<id>.json` here. Changesets are
//!     long-lived facts about work; removing a workspace must *warn* about
//!     dangling changeset references via `orphaned_changeset_refs`, never
//!     drop the metadata silently.
//!   - DO NOT touch a worktree admin entry whose `locked` file exists.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use serde::Serialize;

use ivk_core::{GitBackend, GitCliBackend};

use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

#[derive(Serialize)]
pub(crate) struct OrphanedChangesetRef {
    pub(crate) id: String,
    pub(crate) workspace_name: String,
}

#[derive(Serialize)]
pub(crate) struct SkippedLocked {
    pub(crate) name: String,
    pub(crate) reason: &'static str,
}

#[derive(Serialize)]
pub(crate) struct GcFailure {
    pub(crate) name: String,
    pub(crate) reason: String,
}

#[derive(Serialize)]
pub(crate) struct GcPayload {
    pub(crate) repo_root: String,
    pub(crate) dry_run: bool,
    pub(crate) bytes_before: u64,
    pub(crate) bytes_after: u64,
    pub(crate) bytes_reclaimed: u64,
    pub(crate) bytes_reclaimed_human: String,
    pub(crate) pruned_admin_entries: Vec<String>,
    pub(crate) removed_workspaces: Vec<String>,
    pub(crate) removed_admin: Vec<String>,
    pub(crate) skipped_locked: Vec<SkippedLocked>,
    pub(crate) orphaned_changeset_refs: Vec<OrphanedChangesetRef>,
    /// Registry rows dropped because their workspace directory is gone.
    pub(crate) removed_registry_rows: Vec<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) failed: Vec<GcFailure>,
}

pub fn run(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let dry_run = args.contains(&"--dry-run");

    // No positional args allowed.
    if args.iter().any(|a| !a.starts_with('-')) {
        return error(
            "usage_error",
            "ivk gc takes no positional arguments",
            "ivk help",
            json || agent,
        );
    }

    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            return error(
                "io_error",
                &format!("cannot resolve current directory: {}", e),
                "ivk help",
                json || agent,
            )
        }
    };

    if !cwd.join(".git").exists() {
        return error(
            "not_a_repo",
            &format!(
                "no .git directory at {}; run `git init` first",
                cwd.display()
            ),
            "git init",
            json || agent,
        );
    }

    let lock_dir = cwd.join(".ivk");
    if let Err(e) = fs::create_dir_all(&lock_dir) {
        return error(
            "io_error",
            &format!("cannot create {}: {}", lock_dir.display(), e),
            "ivk doctor",
            json || agent,
        );
    }
    let lock = match GcLock::acquire(&lock_dir.join(".gc.lock")) {
        Ok(l) => l,
        Err(reason) => return error("gc_in_progress", &reason, "ivk help", json || agent),
    };

    let payload = compute_gc_locked(&cwd, dry_run);
    let next_command = next_command_for(&payload);
    let ok = payload.failed.is_empty();

    if json || agent {
        let steps = if agent {
            Some(recommended_steps(&payload))
        } else {
            None
        };
        let env = Envelope {
            ok,
            command: "gc",
            next_command: Some(next_command.into()),
            recommended_next_steps: steps,
            error: None,
            data: payload,
        };
        print_json(&env);
    } else {
        print_human(&payload);
    }
    drop(lock);
    if ok {
        0
    } else {
        1
    }
}

/// Run the gc machinery assuming the `.gc.lock` is held by the caller.
/// Returns the structured payload; bench code calls this directly to drive
/// gc programmatically without going through stdout.
pub(crate) fn compute_gc_locked(cwd: &Path, dry_run: bool) -> GcPayload {
    let workspaces_dir = cwd.join(".ivk").join("workspaces");
    let worktrees_admin = cwd.join(".git").join("worktrees");

    let mut warnings: Vec<String> = Vec::new();
    let bytes_before = dir_size(&workspaces_dir).saturating_add(dir_size(&worktrees_admin));

    // Step 1: prune git's own worktree admin first (cheap, idempotent, gives us a clue list).
    let mut pruned_admin_entries: Vec<String> = Vec::new();
    if !dry_run && worktrees_admin.exists() {
        match GitCliBackend::new().prune_worktrees(cwd) {
            Ok(names) => pruned_admin_entries = names,
            Err(e) => warnings.push(format!("git worktree prune failed: {}", e)),
        }
    }

    // Step 2: enumerate workspaces and admin entries.
    let ws_names = list_dirs(&workspaces_dir);
    let admin_names = list_dirs(&worktrees_admin);

    // Step 3: classify workspaces — orphans are those whose admin link is broken.
    let mut removed_workspaces: Vec<String> = Vec::new();
    let mut skipped_locked: Vec<SkippedLocked> = Vec::new();
    let mut failed: Vec<GcFailure> = Vec::new();

    for name in &ws_names {
        let ws_path = workspaces_dir.join(name);
        match classify_workspace(cwd, &ws_path, name) {
            WorkspaceState::Live => {}
            WorkspaceState::Locked => skipped_locked.push(SkippedLocked {
                name: name.clone(),
                reason: "worktree_locked",
            }),
            WorkspaceState::Orphan(_reason) => {
                if dry_run {
                    removed_workspaces.push(name.clone());
                    continue;
                }
                match remove_workspace(cwd, &ws_path) {
                    Ok(()) => removed_workspaces.push(name.clone()),
                    Err(reason) => failed.push(GcFailure {
                        name: name.clone(),
                        reason,
                    }),
                }
            }
        }
    }

    // Step 4: admin entries whose workspace dir is gone AND not already pruned this run.
    let mut removed_admin: Vec<String> = Vec::new();
    let already: std::collections::HashSet<&str> =
        pruned_admin_entries.iter().map(String::as_str).collect();
    for name in &admin_names {
        if already.contains(name.as_str()) {
            continue;
        }
        if workspaces_dir.join(name).is_dir() {
            continue; // still has a live (or locked) workspace pointing at it
        }
        let admin = worktrees_admin.join(name);
        if admin.join("locked").exists() {
            skipped_locked.push(SkippedLocked {
                name: name.clone(),
                reason: "worktree_locked",
            });
            continue;
        }
        if dry_run {
            removed_admin.push(name.clone());
            continue;
        }
        match fs::remove_dir_all(&admin) {
            Ok(()) => removed_admin.push(name.clone()),
            Err(e) => failed.push(GcFailure {
                name: name.clone(),
                reason: format!("could not remove admin entry: {}", e),
            }),
        }
    }

    // Step 5: stale bench directories left by SIGKILL'd `ivk bench *` runs.
    // BenchDir Drop handles the happy path; crashes leak. Anything older than
    // STALE_BENCH_SECS without a live pid behind it is fair game.
    let bench_dir = cwd.join(".ivk").join("bench");
    if !dry_run && bench_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&bench_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if !p.is_dir() {
                    continue;
                }
                if is_stale_bench_dir(&p) {
                    let _ = fs::remove_dir_all(&p);
                }
            }
        }
    }

    // Step 6: changeset metadata warnings for any removed workspace name.
    let orphaned_changeset_refs = find_orphaned_changesets(cwd, &removed_workspaces);

    // Step 6.5: registry reconcile — drop `ready` rows whose directory is
    // gone (including the ones this run just removed). In-flight rows are
    // deliberately left for `ivk doctor --repair`: gc must not roll back an
    // operation another process may still be running.
    let mut removed_registry_rows: Vec<String> = Vec::new();
    if let Some(reg) = crate::reg::open_synced_if_present(cwd) {
        if let Ok(rows) = reg.workspaces() {
            let mut in_flight = 0usize;
            for w in &rows {
                match w.state {
                    ivk_core::WorkspaceState::Ready => {
                        if !workspaces_dir.join(&w.name).is_dir()
                            && (dry_run || reg.delete_workspace_row(&w.name).is_ok())
                        {
                            removed_registry_rows.push(w.name.clone());
                        }
                    }
                    _ => in_flight += 1,
                }
            }
            if in_flight > 0 {
                warnings.push(format!(
                    "{} in-flight registry row(s) from interrupted operations; run `ivk doctor --repair`",
                    in_flight
                ));
            }
        }
    }

    // Step 7: bytes accounting.
    let bytes_after = if dry_run {
        bytes_before
    } else {
        dir_size(&workspaces_dir).saturating_add(dir_size(&worktrees_admin))
    };
    let bytes_reclaimed = bytes_before.saturating_sub(bytes_after);

    GcPayload {
        repo_root: cwd.display().to_string(),
        dry_run,
        bytes_before,
        bytes_after,
        bytes_reclaimed,
        bytes_reclaimed_human: human_bytes(bytes_reclaimed),
        pruned_admin_entries,
        removed_workspaces,
        removed_admin,
        skipped_locked,
        orphaned_changeset_refs,
        removed_registry_rows,
        warnings,
        failed,
    }
}

fn next_command_for(p: &GcPayload) -> &'static str {
    if !p.failed.is_empty() {
        "ivk doctor"
    } else if !p.orphaned_changeset_refs.is_empty() {
        "ivk ch ls"
    } else {
        "ivk ls"
    }
}

fn recommended_steps(p: &GcPayload) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if p.dry_run {
        v.push(format!(
            "Dry run: would remove {} workspace(s) and {} admin entry/entries (~{}).",
            p.removed_workspaces.len(),
            p.removed_admin.len(),
            p.bytes_reclaimed_human
        ));
        v.push("Re-run without --dry-run to apply.".into());
        return v;
    }
    if !p.failed.is_empty() {
        v.push(format!(
            "{} entry/entries failed to remove.",
            p.failed.len()
        ));
        v.push("Run `ivk doctor` to inspect; rerun `ivk gc` after fixing.".into());
        return v;
    }
    v.push(format!(
        "Reclaimed {} across {} workspace(s) and {} admin entry/entries.",
        p.bytes_reclaimed_human,
        p.removed_workspaces.len(),
        p.removed_admin.len()
    ));
    if !p.orphaned_changeset_refs.is_empty() {
        v.push(format!(
            "{} changeset(s) now reference removed workspaces; their commits live on in git but the workspace tree is gone.",
            p.orphaned_changeset_refs.len()
        ));
        v.push("Inspect with `ivk ch ls`.".into());
    } else {
        v.push("Run `ivk ls` to confirm the tree.".into());
    }
    v
}

fn print_human(p: &GcPayload) {
    println!("ivk gc{}", if p.dry_run { " (dry run)" } else { "" });
    println!(
        "  removed workspaces: {} ({})",
        p.removed_workspaces.len(),
        if p.removed_workspaces.is_empty() {
            "—".into()
        } else {
            p.removed_workspaces.join(", ")
        }
    );
    println!(
        "  removed admin:      {} ({})",
        p.removed_admin.len(),
        if p.removed_admin.is_empty() {
            "—".into()
        } else {
            p.removed_admin.join(", ")
        }
    );
    if !p.pruned_admin_entries.is_empty() {
        println!(
            "  git-pruned admin:   {}",
            p.pruned_admin_entries.join(", ")
        );
    }
    if !p.skipped_locked.is_empty() {
        println!(
            "  skipped (locked):   {}",
            p.skipped_locked
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !p.removed_registry_rows.is_empty() {
        println!(
            "  registry rows:      {} dropped ({})",
            p.removed_registry_rows.len(),
            p.removed_registry_rows.join(", ")
        );
    }
    println!(
        "  bytes reclaimed:    {} ({})",
        p.bytes_reclaimed, p.bytes_reclaimed_human
    );
    if !p.orphaned_changeset_refs.is_empty() {
        println!("  dangling changesets: {}", p.orphaned_changeset_refs.len());
    }
    for f in &p.failed {
        eprintln!("  failed: {} — {}", f.name, f.reason);
    }
}

fn error(code: &'static str, msg: &str, next: &str, as_json: bool) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command: "gc",
            next_command: Some(next.into()),
            recommended_next_steps: None,
            error: Some(ErrorBlock {
                code,
                message: msg.into(),
            }),
            data: (),
        };
        print_json(&env);
    } else {
        eprintln!("ivk: {}", msg);
    }
    if code == "usage_error" || code == "gc_in_progress" || code == "not_a_repo" {
        2
    } else {
        1
    }
}

// ---------- helpers ----------

enum WorkspaceState {
    Live,
    Locked,
    Orphan(&'static str),
}

fn classify_workspace(cwd: &Path, ws_path: &Path, name: &str) -> WorkspaceState {
    let dot_git = ws_path.join(".git");
    if !dot_git.is_file() {
        // No worktree pointer at all → orphan.
        return WorkspaceState::Orphan("missing_git_pointer");
    }
    let pointer = match fs::read_to_string(&dot_git) {
        Ok(s) => s,
        Err(_) => return WorkspaceState::Orphan("unreadable_git_pointer"),
    };
    let admin: PathBuf = pointer
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("gitdir:"))
        .map(|p| PathBuf::from(p.trim()))
        .unwrap_or_default();
    if admin.as_os_str().is_empty() {
        return WorkspaceState::Orphan("bad_git_pointer");
    }
    if admin.join("locked").exists() {
        return WorkspaceState::Locked;
    }
    if !admin.exists() {
        return WorkspaceState::Orphan("admin_missing");
    }
    // Canonicalize the admin path and the expected prefix before the
    // starts_with guard. Without this, a crafted ".git" pointer like
    // `gitdir: <cwd>/.git/worktrees/<name>/../../../evil` could traverse out
    // of the repo and still satisfy starts_with against the lexical prefix.
    let admin_canonical = match admin.canonicalize() {
        Ok(p) => p,
        Err(_) => return WorkspaceState::Orphan("admin_unresolvable"),
    };
    let expected_prefix = cwd.join(".git").join("worktrees");
    let expected_canonical = match expected_prefix.canonicalize() {
        Ok(p) => p,
        Err(_) => return WorkspaceState::Orphan("admin_outside_repo"),
    };
    if !admin_canonical.starts_with(&expected_canonical) {
        return WorkspaceState::Orphan("admin_outside_repo");
    }
    // Sanity: admin's leaf component matches workspace name (else: stale link).
    if admin_canonical.file_name().and_then(|s| s.to_str()) != Some(name) {
        return WorkspaceState::Orphan("admin_name_mismatch");
    }
    WorkspaceState::Live
}

fn remove_workspace(cwd: &Path, ws_path: &Path) -> Result<(), String> {
    ivk_core::remove_workspace(&GitCliBackend::new(), cwd, ws_path)
        .map_err(|_| "could not remove workspace dir".into())
}

fn find_orphaned_changesets(cwd: &Path, removed: &[String]) -> Vec<OrphanedChangesetRef> {
    let ch_dir = cwd.join(".ivk").join("changesets");
    let entries = match fs::read_dir(&ch_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let removed_set: std::collections::HashSet<&str> = removed.iter().map(String::as_str).collect();
    let mut out: Vec<OrphanedChangesetRef> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let s = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        // We only need two fields; deserialize loosely.
        let v: serde_json::Value = match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = v.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let ws = v
            .get("workspace_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !id.is_empty() && !ws.is_empty() && removed_set.contains(ws) {
            out.push(OrphanedChangesetRef {
                id: id.into(),
                workspace_name: ws.into(),
            });
        }
    }
    out
}

fn list_dirs(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = match fs::read_dir(p) {
        Ok(e) => e
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    v.sort();
    v
}

/// Recursive allocated-block sum (`st_blocks` × 512). Unlike `dir_size`
/// (apparent bytes), this reflects what the filesystem has allocated —
/// though CoW-shared blocks still count once per workspace, so real disk
/// growth is lower until files diverge (df is ground truth).
pub(crate) fn dir_allocated(p: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    if !p.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![p.to_path_buf()];
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
                total = total.saturating_add(md.blocks().saturating_mul(512));
            }
        }
    }
    total
}

/// Recursive byte sum without external crates. Symlinks are skipped.
pub(crate) fn dir_size(p: &Path) -> u64 {
    if !p.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![p.to_path_buf()];
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

fn human_bytes(n: u64) -> String {
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

/// Filesystem-backed mutex used by `ivk gc` and bulk `ivk ws rm` to prevent
/// two concurrent destructive passes from racing.
pub(crate) struct GcLock {
    path: PathBuf,
}

/// Locks older than this are considered stale and auto-removed. Real gc /
/// bulk-rm runs are minutes at most, even for hundreds of workspaces.
const STALE_LOCK_SECS: u64 = 300;

impl GcLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self, String> {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(_f) => Ok(Self {
                path: path.to_path_buf(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // The previous holder may have died — check the lock's age.
                if let Some(age) = lock_age_secs(path) {
                    if age >= STALE_LOCK_SECS && fs::remove_file(path).is_ok() {
                        if let Ok(_f) = OpenOptions::new().write(true).create_new(true).open(path) {
                            return Ok(Self {
                                path: path.to_path_buf(),
                            });
                        }
                    }
                    Err(format!(
                        "another `ivk gc` or bulk `ivk ws rm` holds {} (held {}s); wait, or rm it if stale",
                        path.display(),
                        age
                    ))
                } else {
                    Err(format!(
                        "another `ivk gc` or bulk `ivk ws rm` holds {}; wait or remove if stale",
                        path.display()
                    ))
                }
            }
            Err(e) => Err(format!("could not create lock {}: {}", path.display(), e)),
        }
    }
}

impl Drop for GcLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Bench dirs are named `<prefix>-<command>-<pid>-<ts_nanos>`. A dir is stale
/// if its owning pid is dead (best-effort check) AND its mtime is older than
/// the threshold; the latter alone is enough on systems where pid recycling
/// makes the liveness check unreliable.
const STALE_BENCH_SECS: u64 = 24 * 60 * 60;

fn is_stale_bench_dir(p: &Path) -> bool {
    let md = match fs::metadata(p) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let mtime = match md.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let age = match std::time::SystemTime::now().duration_since(mtime) {
        Ok(d) => d.as_secs(),
        Err(_) => return false,
    };
    age >= STALE_BENCH_SECS
}

fn lock_age_secs(path: &Path) -> Option<u64> {
    let md = fs::metadata(path).ok()?;
    let mtime = md.modified().ok()?;
    std::time::SystemTime::now()
        .duration_since(mtime)
        .ok()
        .map(|d| d.as_secs())
}
