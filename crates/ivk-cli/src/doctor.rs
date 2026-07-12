//! `ivk doctor [--agent] [--json]` — the "git status" for ivk.
//!
//! Reports:
//!   - is the cwd inside a git repo? (looks for .git)
//!   - is the cwd inside an ivk workspace? (looks for .git pointer file whose
//!     gitdir points into a workspace admin entry)
//!   - is .ivk/ initialized?
//!   - if inside a workspace, what's its name and git status?
//!
//! Output shape mirrors the convention in `output.rs`. `--agent --json`
//! produces the form the MVP plan §7 specifies.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use ivk_core::{GitBackend, GitCliBackend, Registry, WorkspaceState};

use crate::output::{print_json, wants_agent, wants_json, Envelope};

#[derive(Serialize, Default)]
struct DoctorReport {
    repo_initialized: bool,
    inside_ivk_workspace: bool,
    ivk_dir_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_status: Option<&'static str>, // "clean" | "dirty" | "unknown"
    has_changes: bool,
    repo_root: String,
    strategy: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry: Option<RegistryReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repair: Option<RepairReport>,
}

#[derive(Serialize)]
struct InFlightRow {
    name: String,
    state: &'static str,
}

#[derive(Serialize)]
struct PendingOpRow {
    kind: String,
    workspace_name: String,
}

/// Registry ⇄ directory-layout agreement, computed at the repo root.
#[derive(Serialize)]
struct RegistryReport {
    db_present: bool,
    tracked_workspaces: usize,
    /// Rows journaled `creating` / `removing` — evidence of an interrupted
    /// operation (SIGKILL, crash) that `--repair` rolls back or completes.
    in_flight: Vec<InFlightRow>,
    /// Rows marked `ready` whose directory no longer exists.
    stale_rows: Vec<String>,
    /// Journaled operations that never confirmed completion (e.g. a
    /// `ch new` killed between the commit and the metadata write).
    pending_ops: Vec<PendingOpRow>,
}

#[derive(Serialize)]
struct RepairReport {
    rolled_back: Vec<String>,
    completed_removals: Vec<String>,
    dropped_stale_rows: Vec<String>,
    /// Changesets reconstructed from interrupted `ch new` operations
    /// (the commit had landed; only the metadata write was lost).
    recovered_changesets: Vec<String>,
    cleared_ops: usize,
}

pub fn run(args: &[&str]) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ivk: cannot resolve current directory: {}", e);
            return 1;
        }
    };

    let dot_git = cwd.join(".git");

    // Workspace detection: a `.git` *file* (not directory) with `gitdir: <path>`
    // inside indicates this dir is a worktree. If that path looks like
    // `.../<src>/.git/worktrees/<name>`, we treat <name> as the workspace name.
    let mut inside = false;
    let mut ws_name: Option<String> = None;
    let mut ws_status: Option<&'static str> = None;
    let mut dirty = false;
    if dot_git.is_file() {
        if let Ok(s) = fs::read_to_string(&dot_git) {
            if let Some(line) = s.lines().next() {
                if let Some(rest) = line.strip_prefix("gitdir:") {
                    let admin = PathBuf::from(rest.trim());
                    if let Some(name) = workspace_name_from_admin(&admin) {
                        inside = true;
                        ws_name = Some(name);
                        dirty = GitCliBackend::new()
                            .status(&cwd)
                            .map(|s| s.is_dirty())
                            .unwrap_or(false);
                        ws_status = Some(if dirty { "dirty" } else { "clean" });
                    }
                }
            }
        }
    }

    // Registry health (only meaningful at a repo root that has .ivk/).
    let repair_requested = args.contains(&"--repair");
    let mut registry_report: Option<RegistryReport> = None;
    let mut repair_report: Option<RepairReport> = None;
    if let Some(reg) = crate::reg::open_synced_if_present(&cwd) {
        let ws_dir = cwd.join(".ivk").join("workspaces");
        if repair_requested {
            repair_report = Some(run_repair(&reg, &cwd, &ws_dir));
        }
        registry_report = Some(classify_registry(&reg, &ws_dir));
    }

    let rep = DoctorReport {
        repo_initialized: dot_git.exists(),
        inside_ivk_workspace: inside,
        ivk_dir_present: cwd.join(".ivk").is_dir(),
        workspace_name: ws_name,
        workspace_status: ws_status,
        has_changes: dirty,
        repo_root: cwd.display().to_string(),
        strategy: current_strategy(),
        registry: registry_report,
        repair: repair_report,
    };

    let json = wants_json(args);
    let agent = wants_agent(args);

    if json || agent {
        let next = next_command_hint(&rep);
        let steps = recommended_steps(&rep);
        let env = Envelope {
            ok: true,
            command: "doctor",
            next_command: next,
            recommended_next_steps: if agent { Some(steps) } else { None },
            error: None,
            data: rep,
        };
        print_json(&env);
        return 0;
    }

    // Human-friendly output.
    println!("ivk doctor");
    println!("  repo_initialized:     {}", rep.repo_initialized);
    println!("  ivk_dir_present:      {}", rep.ivk_dir_present);
    println!("  inside_ivk_workspace: {}", rep.inside_ivk_workspace);
    if let Some(n) = rep.workspace_name {
        println!("  workspace_name:       {}", n);
        println!(
            "  workspace_status:     {}",
            rep.workspace_status.unwrap_or("unknown")
        );
    }
    println!("  strategy:             {}", rep.strategy);
    if let Some(r) = &rep.registry {
        println!("  registry:             {} tracked", r.tracked_workspaces);
        if !r.in_flight.is_empty() {
            println!(
                "  in-flight rows:       {} ({}) — run `ivk doctor --repair`",
                r.in_flight.len(),
                r.in_flight
                    .iter()
                    .map(|x| format!("{}:{}", x.name, x.state))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !r.stale_rows.is_empty() {
            println!(
                "  stale rows:           {} ({}) — run `ivk doctor --repair` or `ivk gc`",
                r.stale_rows.len(),
                r.stale_rows.join(", ")
            );
        }
        if !r.pending_ops.is_empty() {
            println!(
                "  pending ops:          {} ({}) — run `ivk doctor --repair`",
                r.pending_ops.len(),
                r.pending_ops
                    .iter()
                    .map(|o| format!("{}:{}", o.kind, o.workspace_name))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    if let Some(fixed) = &rep.repair {
        println!(
            "  repaired:             {} rolled back, {} removals completed, {} stale rows dropped, {} changesets recovered",
            fixed.rolled_back.len(),
            fixed.completed_removals.len(),
            fixed.dropped_stale_rows.len(),
            fixed.recovered_changesets.len()
        );
    }
    if !rep.repo_initialized {
        println!("\nNo git repo here. Initialize with `git init` first.");
    } else if !rep.ivk_dir_present && !rep.inside_ivk_workspace {
        println!("\nTip: run `ivk new <task-name>` to create your first workspace.");
    }
    0
}

fn classify_registry(reg: &Registry, ws_dir: &Path) -> RegistryReport {
    let rows = reg.workspaces().unwrap_or_default();
    let mut in_flight: Vec<InFlightRow> = Vec::new();
    let mut stale_rows: Vec<String> = Vec::new();
    for w in &rows {
        let dir_exists = ws_dir.join(&w.name).is_dir();
        match w.state {
            WorkspaceState::Creating | WorkspaceState::Removing => in_flight.push(InFlightRow {
                name: w.name.clone(),
                state: w.state.as_str(),
            }),
            WorkspaceState::Ready if !dir_exists => stale_rows.push(w.name.clone()),
            WorkspaceState::Ready => {}
        }
    }
    let pending_ops = reg
        .pending_ops()
        .unwrap_or_default()
        .into_iter()
        .map(|op| PendingOpRow {
            kind: op.kind,
            workspace_name: op.workspace_name,
        })
        .collect();
    RegistryReport {
        db_present: true,
        tracked_workspaces: rows.len(),
        in_flight,
        stale_rows,
        pending_ops,
    }
}

/// Complete or roll back interrupted operations, then drop stale rows.
/// A directory that survives the removal attempt (e.g. a locked worktree)
/// keeps its journal row as evidence.
fn run_repair(reg: &Registry, repo_root: &Path, ws_dir: &Path) -> RepairReport {
    let git = GitCliBackend::new();
    let mut report = RepairReport {
        rolled_back: Vec::new(),
        completed_removals: Vec::new(),
        dropped_stale_rows: Vec::new(),
        recovered_changesets: Vec::new(),
        cleared_ops: 0,
    };

    // Pending operations first: a `ch-new` op may reference a workspace the
    // row-repair below would remove, and the committed work is recoverable
    // only while the worktree HEAD still points at it.
    for op in reg.pending_ops().unwrap_or_default() {
        if op.kind == "ch-new" {
            if let Some(id) = recover_changeset(reg, &git, repo_root, ws_dir, &op) {
                report.recovered_changesets.push(id);
            }
        }
        // Unknown kinds have nothing actionable; the row itself is cleared.
        let _ = reg.finish_op(op.id);
        report.cleared_ops += 1;
    }

    for w in reg.workspaces().unwrap_or_default() {
        let ws_path = ws_dir.join(&w.name);
        match w.state {
            WorkspaceState::Creating | WorkspaceState::Removing => {
                if ws_path.exists() {
                    let _ = ivk_core::remove_workspace(&git, repo_root, &ws_path);
                    if ws_path.exists() {
                        continue; // could not clean (e.g. locked); keep the row
                    }
                }
                let _ = reg.delete_workspace_row(&w.name);
                if w.state == WorkspaceState::Creating {
                    report.rolled_back.push(w.name);
                } else {
                    report.completed_removals.push(w.name);
                }
            }
            WorkspaceState::Ready => {
                if !ws_path.is_dir() && reg.delete_workspace_row(&w.name).is_ok() {
                    report.dropped_stale_rows.push(w.name);
                }
            }
        }
    }
    report
}

/// If the journaled `ch new` actually committed (worktree HEAD moved past
/// the recorded base) and no changeset records it, reconstruct the record
/// and its JSON artifact from git facts. Returns the recovered id.
fn recover_changeset(
    reg: &Registry,
    git: &GitCliBackend,
    repo_root: &Path,
    ws_dir: &Path,
    op: &ivk_core::PendingOp,
) -> Option<String> {
    let base = op.base_snapshot.as_deref()?;
    let ws_path = ws_dir.join(&op.workspace_name);
    if !ws_path.is_dir() {
        return None; // workspace gone; nothing to save
    }
    let head = git.resolve_revision(&ws_path, "HEAD").ok()?;
    if head == base {
        return None; // the commit never landed; nothing was lost
    }
    let id = format!("ch_{}", head.get(..12)?);
    if matches!(reg.changeset(&id), Ok(Some(_))) {
        return None; // already recorded (e.g. backfilled from a JSON artifact)
    }
    let touched = git
        .changed_paths(
            repo_root,
            ivk_core::DiffTarget::CommitRange { base, head: &head },
        )
        .unwrap_or_default();
    let rec = ivk_core::ChangesetRecord {
        id: id.clone(),
        workspace_name: op.workspace_name.clone(),
        base_snapshot: base.to_string(),
        result_snapshot: head.clone(),
        touched_paths: touched.clone(),
        created_at_unix: op.started_at_unix,
        exported_branch: None,
        exported_at_unix: None,
    };
    if reg.record_changeset(&rec).is_err() {
        return None;
    }
    // Best-effort JSON artifact, matching the shape ch_new writes.
    let ch_dir = repo_root.join(".ivk").join("changesets");
    if fs::create_dir_all(&ch_dir).is_ok() {
        let body = serde_json::json!({
            "id": id,
            "workspace_name": op.workspace_name,
            "base_snapshot": base,
            "result_snapshot": head,
            "touched_paths": touched,
            "created_at_unix": op.started_at_unix,
        });
        let _ = fs::write(
            ch_dir.join(format!("{}.json", id)),
            serde_json::to_string_pretty(&body).unwrap_or_default(),
        );
    }
    Some(id)
}

fn workspace_name_from_admin(admin: &Path) -> Option<String> {
    // Looking for ".../.git/worktrees/<name>"
    let mut comps = admin.components().rev();
    let name = comps.next()?.as_os_str().to_str()?.to_owned();
    let worktrees = comps.next()?.as_os_str();
    let dot_git = comps.next()?.as_os_str();
    if worktrees == std::ffi::OsStr::new("worktrees") && dot_git == std::ffi::OsStr::new(".git") {
        Some(name)
    } else {
        None
    }
}

/// The materialization strategy the kernel would use here. Comes from the
/// default materializer so doctor/status always agree with what `ivk new`
/// actually reports.
pub fn current_strategy() -> &'static str {
    ivk_core::default_strategy()
}

fn registry_needs_repair(r: &DoctorReport) -> bool {
    r.repair.is_none()
        && r.registry
            .as_ref()
            .map(|reg| {
                !reg.in_flight.is_empty()
                    || !reg.stale_rows.is_empty()
                    || !reg.pending_ops.is_empty()
            })
            .unwrap_or(false)
}

fn next_command_hint(r: &DoctorReport) -> Option<String> {
    if !r.repo_initialized {
        return Some("git init".into());
    }
    if registry_needs_repair(r) {
        return Some("ivk doctor --repair".into());
    }
    if r.inside_ivk_workspace {
        if r.has_changes {
            return Some(format!(
                "ivk ch new {} — once tests pass, record this as a changeset",
                r.workspace_name.as_deref().unwrap_or("<this>")
            ));
        }
        return Some(
            "# you are in a clean workspace; make edits then re-run `ivk doctor --agent --json`"
                .into(),
        );
    }
    if !r.ivk_dir_present {
        return Some("ivk new <task-name>".into());
    }
    Some("ivk new <task-name>".into())
}

fn recommended_steps(r: &DoctorReport) -> Vec<String> {
    if !r.repo_initialized {
        return vec!["Initialize a git repo here first: `git init`".into()];
    }
    if registry_needs_repair(r) {
        let reg = r.registry.as_ref().unwrap();
        return vec![
            format!(
                "{} interrupted operation(s) and {} stale registry row(s) detected.",
                reg.in_flight.len() + reg.pending_ops.len(),
                reg.stale_rows.len()
            ),
            "Run `ivk doctor --repair` to roll back half-created workspaces, complete interrupted removals, and recover committed-but-unrecorded changesets.".into(),
        ];
    }
    if r.inside_ivk_workspace {
        let name = r.workspace_name.clone().unwrap_or_else(|| "<this>".into());
        if r.has_changes {
            return vec![
                "Run project tests inside this workspace.".into(),
                format!("If tests pass, record a changeset: `ivk ch new {}`.", name),
                format!(
                    "If the attempt failed, discard the workspace: `ivk ws rm {}`.",
                    name
                ),
            ];
        }
        return vec![
            "You are inside a clean ivk workspace. Make edits, run tests, then re-run doctor."
                .into(),
        ];
    }
    vec![
        "You are not inside an ivk workspace yet.".into(),
        "Create one for the current task: `ivk new <task-name>`.".into(),
        "Then `cd .ivk/workspaces/<task-name>` and re-run `ivk doctor --agent --json`.".into(),
    ]
}
