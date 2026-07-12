//! `ivk ws ls / show / diff / rm` — Phase 2 lifecycle commands.
//!
//! State source: the directory layout under `.ivk/workspaces/`. Each
//! subdirectory whose name is not reserved is treated as a workspace.
//! `.git` inside the subdir is the worktree pointer file (created by
//! `git worktree add --no-checkout` during `ivk new`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use ivk_core::{DiffTarget, GitBackend, GitCliBackend, Registry};

use crate::gc::GcLock;
use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

#[derive(Serialize)]
struct WorkspaceRow {
    name: String,
    /// Registry lifecycle state (`creating` / `ready` / `removing`).
    state: &'static str,
    status: &'static str,
    has_changes: bool,
    file_count: Option<u64>,
    head: Option<String>,
}

#[derive(Serialize)]
struct LsPayload {
    repo_root: String,
    count: usize,
    workspaces: Vec<WorkspaceRow>,
}

#[derive(Serialize)]
struct ShowPayload {
    repo_root: String,
    name: String,
    path: String,
    head: Option<String>,
    status: &'static str,
    has_changes: bool,
    diff_summary: Option<DiffSummary>,
}

#[derive(Serialize)]
struct DiffSummary {
    files_changed: u32,
    insertions: u32,
    deletions: u32,
}

#[derive(Serialize)]
struct DiffPayload {
    name: String,
    summary: DiffSummary,
    patch: Option<String>,
}

#[derive(Serialize)]
struct RmPayload {
    removed: Vec<String>,
    failed: Vec<RmFailure>,
}

#[derive(Serialize)]
struct RmFailure {
    name: String,
    reason: String,
}

#[derive(Serialize)]
struct RmSkipped {
    name: String,
    reason: String,
}

#[derive(Serialize)]
struct RmBulkPayload {
    selector: &'static str,
    dry_run: bool,
    removed: Vec<String>,
    skipped: Vec<RmSkipped>,
    failed: Vec<RmFailure>,
    // Match gc.rs's accounting fields so agents can chain rm+gc on a uniform shape.
    bytes_before: u64,
    bytes_after: u64,
    bytes_reclaimed: u64,
    bytes_reclaimed_human: String,
}

pub fn ls(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let dir = cwd.join(".ivk").join("workspaces");
    let registry = crate::reg::open_synced_if_present(&cwd);
    let workspaces = read_workspaces(&dir, registry.as_ref());
    let count = workspaces.len();

    if json || agent {
        let next = if count == 0 {
            Some("ivk new <task-name>".into())
        } else {
            Some(format!(
                "ivk ws show {}",
                workspaces
                    .first()
                    .map(|w| w.name.as_str())
                    .unwrap_or("<name>")
            ))
        };
        let env = Envelope {
            ok: true,
            command: "ws.ls",
            next_command: next,
            recommended_next_steps: if agent {
                Some(recommended_for_ls(&workspaces))
            } else {
                None
            },
            error: None,
            data: LsPayload {
                repo_root: cwd.display().to_string(),
                count,
                workspaces,
            },
        };
        print_json(&env);
    } else if count == 0 {
        println!("0 workspaces. Create one with `ivk new <task-name>`.");
    } else {
        println!("{} workspace(s):", count);
        for w in &workspaces {
            println!(
                "  {:<28} {:<7} head={}",
                w.name,
                w.status,
                w.head.as_deref().unwrap_or("?")
            );
        }
    }
    0
}

pub fn show(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let name = match positional(args) {
        Some(n) => n,
        None => {
            return ws_error(
                "ws.show",
                "missing_argument",
                "ws show requires a workspace name",
                "ivk ls",
                json || agent,
            )
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(".ivk").join("workspaces").join(name);
    if !path.is_dir() {
        return ws_error(
            "ws.show",
            "not_found",
            &format!("no workspace named `{}`", name),
            "ivk ls",
            json || agent,
        );
    }
    let head = git_short_head(&path);
    let (status, dirty) = git_status_in(&path);
    let diff_summary = git_diff_stat(&path);

    if json || agent {
        let env = Envelope {
            ok: true,
            command: "ws.show",
            next_command: Some(if dirty {
                format!(
                    "cd {} && # iterate; once tests pass: `ivk ch new {}`",
                    path.display(),
                    name
                )
            } else {
                format!(
                    "cd {} && # workspace is clean, ready to edit",
                    path.display()
                )
            }),
            recommended_next_steps: if agent {
                Some(if dirty {
                    vec![
                        "Workspace has uncommitted changes.".into(),
                        format!("Record a changeset: `ivk ch new {}`.", name),
                        format!("Or discard the attempt: `ivk ws rm {}`.", name),
                    ]
                } else {
                    vec!["Workspace is clean. cd in and start editing.".into()]
                })
            } else {
                None
            },
            error: None,
            data: ShowPayload {
                repo_root: cwd.display().to_string(),
                name: name.to_string(),
                path: path.display().to_string(),
                head,
                status,
                has_changes: dirty,
                diff_summary,
            },
        };
        print_json(&env);
    } else {
        println!("workspace: {}", name);
        println!("  path:    {}", path.display());
        println!("  head:    {}", head.as_deref().unwrap_or("?"));
        println!("  status:  {}", status);
        if let Some(d) = &diff_summary {
            println!(
                "  diff:    {} files changed, +{} -{}",
                d.files_changed, d.insertions, d.deletions
            );
        }
    }
    0
}

pub fn diff(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let name = match positional(args) {
        Some(n) => n,
        None => {
            return ws_error(
                "ws.diff",
                "missing_argument",
                "ws diff requires a workspace name",
                "ivk ls",
                json || agent,
            )
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = cwd.join(".ivk").join("workspaces").join(name);
    if !path.is_dir() {
        return ws_error(
            "ws.diff",
            "not_found",
            &format!("no workspace named `{}`", name),
            "ivk ls",
            json || agent,
        );
    }

    let summary = git_diff_stat(&path).unwrap_or(DiffSummary {
        files_changed: 0,
        insertions: 0,
        deletions: 0,
    });
    let patch = GitCliBackend::new()
        .diff_patch(&path, DiffTarget::WorktreeToHead, false)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned());

    if json || agent {
        let env = Envelope {
            ok: true,
            command: "ws.diff",
            next_command: None,
            recommended_next_steps: None,
            error: None,
            data: DiffPayload {
                name: name.to_string(),
                summary,
                patch,
            },
        };
        print_json(&env);
    } else if let Some(p) = patch {
        print!("{}", p);
    }
    0
}

#[derive(Serialize)]
struct DuRow {
    name: String,
    apparent_bytes: u64,
    apparent_human: String,
    allocated_bytes: u64,
    allocated_human: String,
}

#[derive(Serialize)]
struct DuPayload {
    count: usize,
    total_apparent_bytes: u64,
    total_apparent_human: String,
    total_allocated_bytes: u64,
    total_allocated_human: String,
    /// CoW caveat: shared blocks count once per workspace here; actual disk
    /// growth is lower until files diverge.
    note: &'static str,
    workspaces: Vec<DuRow>,
}

const DU_NOTE: &str = "allocated counts CoW-shared blocks once per workspace; \
real disk growth is lower until files diverge (df is ground truth)";

pub fn du(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let names: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let workspaces_dir = cwd.join(".ivk").join("workspaces");

    let targets: Vec<String> = if names.is_empty() {
        list_workspace_names(&workspaces_dir)
    } else {
        for name in &names {
            if !workspaces_dir.join(name).is_dir() {
                return ws_error(
                    "ws.du",
                    "not_found",
                    &format!("no workspace named `{}`", name),
                    "ivk ls",
                    json || agent,
                );
            }
        }
        names.iter().map(|n| n.to_string()).collect()
    };

    let mut rows: Vec<DuRow> = targets
        .iter()
        .map(|name| {
            let p = workspaces_dir.join(name);
            let apparent = crate::gc::dir_size(&p);
            let allocated = crate::gc::dir_allocated(&p);
            DuRow {
                name: name.clone(),
                apparent_bytes: apparent,
                apparent_human: human_bytes(apparent),
                allocated_bytes: allocated,
                allocated_human: human_bytes(allocated),
            }
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.allocated_bytes));

    let total_apparent: u64 = rows.iter().map(|r| r.apparent_bytes).sum();
    let total_allocated: u64 = rows.iter().map(|r| r.allocated_bytes).sum();

    if json || agent {
        let steps = if agent {
            Some(if rows.is_empty() {
                vec!["No workspaces. Create one with `ivk new <task-name>`.".into()]
            } else {
                vec![
                    format!(
                        "{} workspace(s), {} apparent / {} allocated in total.",
                        rows.len(),
                        human_bytes(total_apparent),
                        human_bytes(total_allocated)
                    ),
                    format!(
                        "Largest: `{}` ({} allocated). Discard finished attempts with `ivk ws rm <name>`, then `ivk gc`.",
                        rows[0].name, rows[0].allocated_human
                    ),
                ]
            })
        } else {
            None
        };
        let env = Envelope {
            ok: true,
            command: "ws.du",
            next_command: Some(if rows.is_empty() {
                "ivk new <task-name>".into()
            } else {
                "ivk gc".into()
            }),
            recommended_next_steps: steps,
            error: None,
            data: DuPayload {
                count: rows.len(),
                total_apparent_bytes: total_apparent,
                total_apparent_human: human_bytes(total_apparent),
                total_allocated_bytes: total_allocated,
                total_allocated_human: human_bytes(total_allocated),
                note: DU_NOTE,
                workspaces: rows,
            },
        };
        print_json(&env);
    } else if rows.is_empty() {
        println!("0 workspaces. Create one with `ivk new <task-name>`.");
    } else {
        println!("{:<28} {:>12} {:>12}", "workspace", "apparent", "allocated");
        for r in &rows {
            println!(
                "{:<28} {:>12} {:>12}",
                r.name, r.apparent_human, r.allocated_human
            );
        }
        println!(
            "{:<28} {:>12} {:>12}",
            "total",
            human_bytes(total_apparent),
            human_bytes(total_allocated)
        );
        println!("note: {}", DU_NOTE);
    }
    0
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BulkMode {
    All,
    Exported,
}

pub fn rm(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);

    let has_all = args.contains(&"--all");
    let has_exported = args.contains(&"--exported");
    let has_failed = args.contains(&"--failed");
    let has_all_discarded = args.contains(&"--all-discarded");
    let has_yes = args.contains(&"--yes");
    let has_force = args.contains(&"--force");
    let has_dry_run = args.contains(&"--dry-run");

    // Deferred flags: refuse explicitly and point at a supported alternative.
    if has_failed {
        return bulk_error(
            "unsupported_flag",
            "--failed requires test-result tracking which is not implemented in 0.0.1",
            "ivk ws rm --all --yes",
            json || agent,
        );
    }
    if has_all_discarded {
        return bulk_error(
            "unsupported_flag",
            "--all-discarded requires an exported/discarded marker not present in 0.0.1; use --exported or --all",
            "ivk ws rm --exported --yes",
            json || agent,
        );
    }

    let names: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();

    // Mutually-exclusive selectors.
    if has_all && has_exported {
        return bulk_error(
            "conflicting_args",
            "--all and --exported cannot be combined",
            "ivk help",
            json || agent,
        );
    }
    if (has_all || has_exported) && !names.is_empty() {
        return bulk_error(
            "conflicting_args",
            "positional names cannot be combined with --all or --exported",
            "ivk help",
            json || agent,
        );
    }

    if has_all || has_exported {
        let mode = if has_all {
            BulkMode::All
        } else {
            BulkMode::Exported
        };
        return rm_bulk(mode, has_yes, has_force, has_dry_run, json, agent);
    }

    if names.is_empty() {
        return ws_error(
            "ws.rm",
            "missing_argument",
            "ws rm requires at least one workspace name (or --all / --exported)",
            "ivk ls",
            json || agent,
        );
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let workspaces_dir = cwd.join(".ivk").join("workspaces");
    let registry = crate::reg::open_synced_if_present(&cwd);

    let mut removed: Vec<String> = Vec::new();
    let mut failed: Vec<RmFailure> = Vec::new();
    for name in &names {
        let path = workspaces_dir.join(name);
        if !path.is_dir() {
            failed.push(RmFailure {
                name: (*name).to_string(),
                reason: "not found".into(),
            });
            continue;
        }
        // Journal the removal; an interrupted run leaves a `removing` row
        // that `ivk doctor --repair` completes.
        if let Some(reg) = &registry {
            let _ = reg.begin_remove(name);
        }
        match rm_one(&cwd, &path) {
            Ok(()) => {
                if let Some(reg) = &registry {
                    let _ = reg.finish_remove(name);
                }
                removed.push((*name).to_string())
            }
            Err(reason) => failed.push(RmFailure {
                name: (*name).to_string(),
                reason,
            }),
        }
    }

    let ok = failed.is_empty();
    if json || agent {
        let steps = if agent {
            Some(if ok {
                vec![
                    format!("Removed {} workspace(s).", removed.len()),
                    "Run `ivk gc` to reclaim worktree admin disk.".into(),
                ]
            } else {
                vec![
                    format!("Failed to remove {} workspace(s).", failed.len()),
                    "Inspect with `ivk doctor --agent --json` and retry the named workspace."
                        .into(),
                ]
            })
        } else {
            None
        };
        let env = Envelope {
            ok,
            command: "ws.rm",
            next_command: Some(if ok {
                "ivk gc".into()
            } else {
                "ivk doctor".into()
            }),
            recommended_next_steps: steps,
            error: None,
            data: RmPayload { removed, failed },
        };
        print_json(&env);
    } else {
        for n in &removed {
            println!("removed {}", n);
        }
        for f in &failed {
            eprintln!("failed to remove {}: {}", f.name, f.reason);
        }
    }
    if ok {
        0
    } else {
        1
    }
}

fn rm_one(cwd: &Path, ws_path: &Path) -> Result<(), String> {
    ivk_core::remove_workspace(&GitCliBackend::new(), cwd, ws_path)
        .map_err(|_| "could not remove".into())
}

fn rm_bulk(mode: BulkMode, yes: bool, force: bool, dry_run: bool, json: bool, agent: bool) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if !cwd.join(".git").exists() {
        return bulk_error(
            "not_a_repo",
            "no .git here; run `git init` first",
            "git init",
            json || agent,
        );
    }
    let workspaces_dir = cwd.join(".ivk").join("workspaces");
    if let Err(e) = fs::create_dir_all(cwd.join(".ivk")) {
        return bulk_error(
            "io_error",
            &format!("cannot create .ivk/: {}", e),
            "ivk doctor",
            json || agent,
        );
    }
    let _lock = match GcLock::acquire(&cwd.join(".ivk").join(".gc.lock")) {
        Ok(l) => l,
        Err(reason) => return bulk_error("gc_in_progress", &reason, "ivk help", json || agent),
    };

    let mut ws_names = list_workspace_names(&workspaces_dir);
    ws_names.sort();

    let candidates: Vec<String> = match mode {
        BulkMode::All => ws_names.clone(),
        BulkMode::Exported => {
            let refs = agent_refs(&cwd);
            ws_names
                .iter()
                .filter(|name| {
                    let head = workspace_head(&workspaces_dir.join(name));
                    match (refs.get(name.as_str()), head) {
                        (Some(branch_sha), Some(head_sha)) => *branch_sha == head_sha,
                        _ => false,
                    }
                })
                .cloned()
                .collect()
        }
    };

    let selector: &'static str = match mode {
        BulkMode::All => "all",
        BulkMode::Exported => "exported",
    };

    if candidates.is_empty() {
        let payload = RmBulkPayload {
            selector,
            dry_run,
            removed: vec![],
            skipped: vec![],
            failed: vec![],
            bytes_before: 0,
            bytes_after: 0,
            bytes_reclaimed: 0,
            bytes_reclaimed_human: "0 B".into(),
        };
        emit_bulk_envelope(&payload, json, agent);
        return 0;
    }

    let bytes_before = crate::gc::dir_size(&workspaces_dir)
        .saturating_add(crate::gc::dir_size(&cwd.join(".git").join("worktrees")));

    if !yes {
        return bulk_error(
            "confirmation_required",
            &format!(
                "refusing to remove {} workspace(s) without --yes",
                candidates.len()
            ),
            &format!("ivk ws rm --{} --yes", selector),
            json || agent,
        );
    }

    let mut removed: Vec<String> = Vec::new();
    let mut skipped: Vec<RmSkipped> = Vec::new();
    let mut failed: Vec<RmFailure> = Vec::new();
    let registry = crate::reg::open_synced_if_present(&cwd);

    for name in &candidates {
        let ws_path = workspaces_dir.join(name);
        // Locked guard. Never overridable — locked worktrees are held by an
        // external process; --force only overrides the dirty guard (uncommitted
        // edits), not the locked-worktree contract (gc.rs file-level comment).
        if worktree_locked(&cwd, name) {
            skipped.push(RmSkipped {
                name: name.clone(),
                reason: "worktree_locked".into(),
            });
            continue;
        }
        // Dirty / unknown-status guard. git_status_in returns ("unknown", false)
        // when git itself fails (broken pointer, missing admin); treat unknown
        // as "we don't know it's safe" — only --force should override.
        let (status, dirty) = git_status_in(&ws_path);
        if dirty && !force {
            skipped.push(RmSkipped {
                name: name.clone(),
                reason: "dirty, pass --force to override".into(),
            });
            continue;
        }
        if status == "unknown" && !force {
            skipped.push(RmSkipped {
                name: name.clone(),
                reason: "unknown git status, pass --force to override".into(),
            });
            continue;
        }
        if dry_run {
            removed.push(name.clone());
            continue;
        }
        if let Some(reg) = &registry {
            let _ = reg.begin_remove(name);
        }
        match rm_one(&cwd, &ws_path) {
            Ok(()) => {
                if let Some(reg) = &registry {
                    let _ = reg.finish_remove(name);
                }
                removed.push(name.clone())
            }
            Err(reason) => failed.push(RmFailure {
                name: name.clone(),
                reason,
            }),
        }
    }

    if !dry_run {
        let _ = GitCliBackend::new().prune_worktrees(&cwd);
    }

    let bytes_after = if dry_run {
        bytes_before
    } else {
        crate::gc::dir_size(&workspaces_dir)
            .saturating_add(crate::gc::dir_size(&cwd.join(".git").join("worktrees")))
    };
    let bytes_reclaimed = bytes_before.saturating_sub(bytes_after);
    let payload = RmBulkPayload {
        selector,
        dry_run,
        removed,
        skipped,
        failed,
        bytes_before,
        bytes_after,
        bytes_reclaimed,
        bytes_reclaimed_human: human_bytes(bytes_reclaimed),
    };
    let exit = if payload.failed.is_empty() { 0 } else { 1 };
    emit_bulk_envelope(&payload, json, agent);
    exit
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

fn emit_bulk_envelope(p: &RmBulkPayload, json: bool, agent: bool) {
    if !(json || agent) {
        for n in &p.removed {
            println!(
                "{} {}",
                if p.dry_run { "would remove" } else { "removed" },
                n
            );
        }
        for s in &p.skipped {
            println!("skipped {} ({})", s.name, s.reason);
        }
        for f in &p.failed {
            eprintln!("failed to remove {}: {}", f.name, f.reason);
        }
        return;
    }
    let next_command_owned: String = if !p.failed.is_empty() {
        "ivk doctor".into()
    } else if !p.removed.is_empty() {
        "ivk gc".into()
    } else if !p.skipped.is_empty() {
        // Nothing removed but candidates were skipped → tell the agent how to retry.
        format!("ivk ws rm --{} --yes --force", p.selector)
    } else {
        "ivk ls".into()
    };
    let steps = if agent {
        let mut v: Vec<String> = Vec::new();
        if p.dry_run {
            v.push(format!(
                "Dry run: would remove {} workspace(s).",
                p.removed.len()
            ));
            v.push(format!("Re-run with `--{} --yes` to apply.", p.selector));
        } else {
            v.push(format!("Removed {} workspace(s).", p.removed.len()));
            if !p.skipped.is_empty() {
                v.push(format!(
                    "{} skipped — pass --force to override.",
                    p.skipped.len()
                ));
            }
            v.push("Run `ivk gc` to reclaim worktree admin disk.".into());
        }
        Some(v)
    } else {
        None
    };
    let env = Envelope {
        ok: p.failed.is_empty(),
        command: "ws.rm",
        next_command: Some(next_command_owned),
        recommended_next_steps: steps,
        error: None,
        data: p,
    };
    print_json(&env);
}

fn bulk_error(code: &'static str, msg: &str, next: &str, as_json: bool) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command: "ws.rm",
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
    exit_for(code)
}

fn list_workspace_names(dir: &Path) -> Vec<String> {
    match fs::read_dir(dir) {
        Ok(e) => e
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn workspace_head(ws_path: &Path) -> Option<String> {
    GitCliBackend::new().resolve_revision(ws_path, "HEAD").ok()
}

fn agent_refs(cwd: &Path) -> HashMap<String, String> {
    let refs = GitCliBackend::new()
        .list_refs(cwd, "refs/heads/agent/")
        .unwrap_or_default();
    refs.into_iter()
        .filter_map(|r| {
            r.name
                .strip_prefix("agent/")
                .map(|name| (name.to_string(), r.sha))
        })
        .collect()
}

fn worktree_locked(cwd: &Path, name: &str) -> bool {
    cwd.join(".git")
        .join("worktrees")
        .join(name)
        .join("locked")
        .exists()
}

fn ws_error(
    command: &'static str,
    code: &'static str,
    msg: &str,
    next: &str,
    as_json: bool,
) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command,
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
    exit_for(code)
}

/// Spec: `2` for usage / refusal / conflict / unsupported flags; `1` for
/// runtime failures (io, partial removal, not-found). Mirrors gc.rs.
fn exit_for(code: &str) -> i32 {
    match code {
        "usage_error"
        | "gc_in_progress"
        | "not_a_repo"
        | "confirmation_required"
        | "unsupported_flag"
        | "conflicting_args"
        | "missing_argument" => 2,
        _ => 1,
    }
}

fn positional<'a>(args: &'a [&'a str]) -> Option<&'a str> {
    args.iter().copied().find(|a| !a.starts_with('-'))
}

fn read_workspaces(dir: &Path, registry: Option<&Registry>) -> Vec<WorkspaceRow> {
    // The directory layout stays the source of files; the registry is the
    // source of lifecycle state. Dirs without a row (registry unavailable)
    // read as `ready`.
    let states: HashMap<String, &'static str> = registry
        .and_then(|r| r.workspaces().ok())
        .map(|rows| {
            rows.into_iter()
                .map(|w| (w.name, w.state.as_str()))
                .collect()
        })
        .unwrap_or_default();

    let mut rows: Vec<WorkspaceRow> = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return rows;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let head = git_short_head(&p);
        let (status, dirty) = git_status_in(&p);
        rows.push(WorkspaceRow {
            state: states.get(&name).copied().unwrap_or("ready"),
            name,
            status,
            has_changes: dirty,
            file_count: None, // expensive; show command computes on demand
            head,
        });
    }
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

fn recommended_for_ls(rows: &[WorkspaceRow]) -> Vec<String> {
    if rows.is_empty() {
        return vec!["No workspaces. Create one with `ivk new <task-name>`.".into()];
    }
    let dirty: Vec<&str> = rows
        .iter()
        .filter(|w| w.has_changes)
        .map(|w| w.name.as_str())
        .collect();
    if dirty.is_empty() {
        vec!["All workspaces clean. cd into one to start editing.".into()]
    } else {
        vec![
            format!(
                "{} workspace(s) have uncommitted changes: {}",
                dirty.len(),
                dirty.join(", ")
            ),
            "For each: `ivk ch new <name>` if good, `ivk ws rm <name>` if not.".into(),
        ]
    }
}

fn git_short_head(p: &Path) -> Option<String> {
    GitCliBackend::new().resolve_revision_short(p, "HEAD").ok()
}

fn git_status_in(p: &Path) -> (&'static str, bool) {
    match GitCliBackend::new().status(p) {
        Ok(s) if s.is_dirty() => ("dirty", true),
        Ok(_) => ("clean", false),
        Err(_) => ("unknown", false),
    }
}

fn git_diff_stat(p: &Path) -> Option<DiffSummary> {
    let stat = GitCliBackend::new()
        .diff_stat(p, DiffTarget::WorktreeToHead)
        .ok()?;
    Some(DiffSummary {
        files_changed: stat.files_changed,
        insertions: stat.insertions,
        deletions: stat.deletions,
    })
}
