//! `ivk ch new / ls / show`, `ivk export`, `ivk patch` — Phase 3.
//!
//! ChangeSet model (v0.0.1):
//!   - A workspace is a normal git worktree on a detached HEAD.
//!   - `ivk ch new <ws>` runs `git add -A && git commit -m "..."` *inside* the
//!     worktree, which advances the worktree's HEAD. The new commit lives in
//!     the source repo's object store (worktrees share it).
//!   - Metadata is written to `.ivk/changesets/<id>.json` so the changeset is
//!     discoverable without scanning git refs.
//!   - `ivk export <id> [<branch>]` creates a git branch in the source repo
//!     pointing at the changeset commit. The branch is just a normal ref;
//!     `git push origin <branch>` works.
//!
//! `ivk ship` (the all-in-one variant covering push + gh pr create) is
//! intentionally not implemented yet — it requires the `gh` CLI and we want
//! a separate spike on PR conventions before locking the workflow.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use ivk_core::{CommitIdentity, DiffTarget, GitBackend, GitCliBackend};

use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

#[derive(Serialize, Deserialize)]
struct Changeset {
    id: String,
    workspace_name: String,
    base_snapshot: String,   // git sha the workspace started from
    result_snapshot: String, // git sha after the auto-commit
    touched_paths: Vec<String>,
    created_at_unix: u64,
    // Export stamps live in the registry; absent in the JSON artifacts
    // written by ch_new and in pre-Phase-B files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exported_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exported_at_unix: Option<u64>,
}

impl Changeset {
    fn to_record(&self) -> ivk_core::ChangesetRecord {
        ivk_core::ChangesetRecord {
            id: self.id.clone(),
            workspace_name: self.workspace_name.clone(),
            base_snapshot: self.base_snapshot.clone(),
            result_snapshot: self.result_snapshot.clone(),
            touched_paths: self.touched_paths.clone(),
            created_at_unix: self.created_at_unix,
            exported_branch: self.exported_branch.clone(),
            exported_at_unix: self.exported_at_unix,
        }
    }

    fn from_record(r: ivk_core::ChangesetRecord) -> Self {
        Changeset {
            id: r.id,
            workspace_name: r.workspace_name,
            base_snapshot: r.base_snapshot,
            result_snapshot: r.result_snapshot,
            touched_paths: r.touched_paths,
            created_at_unix: r.created_at_unix,
            exported_branch: r.exported_branch,
            exported_at_unix: r.exported_at_unix,
        }
    }
}

enum LoadError {
    NotFound,
    BadMetadata(String),
}

/// Changeset lookup: registry first (backfilled from JSON on open), JSON
/// file as the fallback so a repo with a broken/absent db still works.
fn load_changeset(cwd: &Path, id: &str) -> Result<Changeset, LoadError> {
    if let Some(reg) = crate::reg::open_synced_if_present(cwd) {
        if let Ok(Some(rec)) = reg.changeset(id) {
            return Ok(Changeset::from_record(rec));
        }
    }
    let path = cwd
        .join(".ivk")
        .join("changesets")
        .join(format!("{}.json", id));
    let body = fs::read_to_string(&path).map_err(|_| LoadError::NotFound)?;
    serde_json::from_str(&body).map_err(|e| LoadError::BadMetadata(e.to_string()))
}

#[derive(Serialize)]
struct ChNewPayload {
    #[serde(flatten)]
    changeset: Changeset,
    files_changed: u32,
    insertions: u32,
    deletions: u32,
}

#[derive(Serialize)]
struct ChLsPayload {
    count: usize,
    changesets: Vec<Changeset>,
}

#[derive(Serialize)]
struct ChCheckPayload {
    changeset_id: String,
    target_ref: String,
    target_snapshot: String,
    clean: bool,
    conflict_paths: Vec<String>,
    /// Tree oid of the merge result — with `clean`, a future integration
    /// step can `git commit-tree` it without redoing the merge.
    merged_tree: String,
}

#[derive(Serialize)]
struct ExportPayload {
    changeset_id: String,
    branch: String,
    sha: String,
}

#[derive(Serialize)]
struct PatchPayload {
    changeset_id: String,
    output_path: String,
    bytes_written: u64,
}

pub fn ch_new(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let name = match positional(args) {
        Some(n) => n,
        None => {
            return ch_error(
                "ch.new",
                "missing_argument",
                "ch new requires a workspace name",
                json || agent,
            )
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ws_path = cwd.join(".ivk").join("workspaces").join(name);
    if !ws_path.is_dir() {
        return ch_error(
            "ch.new",
            "workspace_not_found",
            &format!("no workspace named `{}`", name),
            json || agent,
        );
    }

    let git = GitCliBackend::new();
    let base_snapshot = match git.resolve_revision(&ws_path, "HEAD") {
        Ok(s) => s,
        Err(_) => {
            return ch_error(
                "ch.new",
                "git_rev_parse_failed",
                "could not read workspace HEAD",
                json || agent,
            )
        }
    };

    // Are there changes to commit?
    let touched: Vec<String> = git
        .status(&ws_path)
        .map(|s| s.touched_paths())
        .unwrap_or_default();
    if touched.is_empty() {
        return ch_error(
            "ch.new",
            "no_changes",
            &format!("workspace `{}` has no uncommitted changes", name),
            json || agent,
        );
    }

    // Journal the intent before committing: a kill between the commit and
    // the metadata write below leaves this row, and `ivk doctor --repair`
    // reconstructs the changeset from it (base = the HEAD recorded here).
    let registry = crate::reg::open_synced(&cwd);
    let op_id = registry
        .as_ref()
        .and_then(|r| r.begin_op("ch-new", name, Some(&base_snapshot)).ok());

    // Commit inside the worktree — the kernel's one explicit committing op.
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let msg = format!("ivk: changeset from workspace `{}` at {}", name, stamp);
    let result_snapshot =
        match git.stage_all_and_commit(&ws_path, &msg, &CommitIdentity::ivk_default()) {
            Ok(sha) => sha,
            Err(e) => {
                // add/commit failed → HEAD unchanged → clear the journal row.
                // rev-parse failed AFTER a successful commit → keep the row
                // so `doctor --repair` can reconstruct the changeset.
                if e.op != "rev-parse" {
                    if let (Some(reg), Some(op)) = (&registry, op_id) {
                        let _ = reg.finish_op(op);
                    }
                }
                let (code, message): (&'static str, &str) = match e.op {
                    "add" => ("git_add_failed", "git add -A failed"),
                    "commit" => ("git_commit_failed", "git commit failed"),
                    _ => ("git_rev_parse_failed", "could not read post-commit HEAD"),
                };
                return ch_error("ch.new", code, message, json || agent);
            }
        };

    let id = format!("ch_{}", &result_snapshot[..12]);
    let changeset = Changeset {
        id: id.clone(),
        workspace_name: name.to_string(),
        base_snapshot,
        result_snapshot: result_snapshot.clone(),
        touched_paths: touched,
        created_at_unix: stamp,
        exported_branch: None,
        exported_at_unix: None,
    };

    // Persist metadata.
    let ch_dir = cwd.join(".ivk").join("changesets");
    if let Err(e) = fs::create_dir_all(&ch_dir) {
        return ch_error(
            "ch.new",
            "io_error",
            &format!("could not create {}: {}", ch_dir.display(), e),
            json || agent,
        );
    }
    let meta_path = ch_dir.join(format!("{}.json", id));
    let body = serde_json::to_string_pretty(&changeset).unwrap();
    if let Err(e) = fs::write(&meta_path, body) {
        return ch_error(
            "ch.new",
            "io_error",
            &format!("could not write {}: {}", meta_path.display(), e),
            json || agent,
        );
    }

    // Registry row (the JSON file above remains the portable artifact),
    // then confirm the journaled operation as complete.
    if let Some(reg) = &registry {
        let _ = reg.record_changeset(&changeset.to_record());
        if let Some(op) = op_id {
            let _ = reg.finish_op(op);
        }
    }

    // Pull a shortstat for the response.
    let (files_changed, insertions, deletions) =
        diff_stat_between(&cwd, &changeset.base_snapshot, &changeset.result_snapshot);

    if json || agent {
        let env = Envelope {
            ok: true,
            command: "ch.new",
            next_command: Some(format!("ivk ch check {}", id)),
            recommended_next_steps: if agent {
                Some(vec![
                    format!("Changeset {} created from workspace {}.", id, name),
                    format!(
                        "Check it merges cleanly: `ivk ch check {}`, then export: `ivk export {} agent/{}`.",
                        id, id, name
                    ),
                ])
            } else {
                None
            },
            error: None,
            data: ChNewPayload {
                changeset,
                files_changed,
                insertions,
                deletions,
            },
        };
        print_json(&env);
    } else {
        println!(
            "created changeset {} from workspace {} ({} files, +{} -{})",
            id, name, files_changed, insertions, deletions
        );
        println!("  next: ivk ch check {}", id);
    }
    0
}

pub fn ch_ls(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Registry first (backfilled from the JSON artifacts on open); scan the
    // JSON files directly only when no registry is available.
    let mut changesets: Vec<Changeset> = match crate::reg::open_synced_if_present(&cwd) {
        Some(reg) => reg
            .changesets()
            .unwrap_or_default()
            .into_iter()
            .map(Changeset::from_record)
            .collect(),
        None => {
            let ch_dir = cwd.join(".ivk").join("changesets");
            let mut v: Vec<Changeset> = Vec::new();
            if let Ok(entries) = fs::read_dir(&ch_dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|s| s.to_str()) != Some("json") {
                        continue;
                    }
                    if let Ok(s) = fs::read_to_string(&p) {
                        if let Ok(c) = serde_json::from_str::<Changeset>(&s) {
                            v.push(c);
                        }
                    }
                }
            }
            v
        }
    };
    changesets.sort_by_key(|c| std::cmp::Reverse(c.created_at_unix));

    if json || agent {
        let env = Envelope {
            ok: true,
            command: "ch.ls",
            next_command: if changesets.is_empty() {
                Some("ivk new <task-name>".into())
            } else {
                Some(format!("ivk export {} agent/<task-name>", changesets[0].id))
            },
            recommended_next_steps: None,
            error: None,
            data: ChLsPayload {
                count: changesets.len(),
                changesets,
            },
        };
        print_json(&env);
    } else if changesets.is_empty() {
        println!("0 changesets. Make one with `ivk ch new <workspace-name>`.");
    } else {
        println!("{} changeset(s):", changesets.len());
        for c in &changesets {
            println!(
                "  {:<20} ws={:<24} -> {}",
                c.id,
                c.workspace_name,
                &c.result_snapshot[..12]
            );
        }
    }
    0
}

pub fn ch_show(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let id = match positional(args) {
        Some(n) => n,
        None => {
            return ch_error(
                "ch.show",
                "missing_argument",
                "ch show requires a changeset id",
                json || agent,
            )
        }
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let c = match load_changeset(&cwd, id) {
        Ok(c) => c,
        Err(LoadError::NotFound) => {
            return ch_error(
                "ch.show",
                "not_found",
                &format!("no changeset `{}`", id),
                json || agent,
            )
        }
        Err(LoadError::BadMetadata(e)) => {
            return ch_error(
                "ch.show",
                "bad_metadata",
                &format!("invalid changeset metadata: {}", e),
                json || agent,
            )
        }
    };
    if json || agent {
        let env = Envelope {
            ok: true,
            command: "ch.show",
            next_command: Some(format!("ivk export {} agent/{}", c.id, c.workspace_name)),
            recommended_next_steps: None,
            error: None,
            data: c,
        };
        print_json(&env);
    } else {
        println!("changeset: {}", c.id);
        println!("  workspace:        {}", c.workspace_name);
        println!("  base_snapshot:    {}", c.base_snapshot);
        println!("  result_snapshot:  {}", c.result_snapshot);
        println!("  touched ({} files):", c.touched_paths.len());
        for f in c.touched_paths.iter().take(10) {
            println!("    {}", f);
        }
        if c.touched_paths.len() > 10 {
            println!("    ... and {} more", c.touched_paths.len() - 10);
        }
    }
    0
}

#[derive(Serialize)]
struct HotspotRow {
    path: String,
    changeset_count: u64,
    workspace_count: u64,
}

#[derive(Serialize)]
struct HotspotsPayload {
    count: usize,
    min_changesets: u32,
    hotspots: Vec<HotspotRow>,
}

/// `ivk ch hotspots [--top N] [--min N]` — files that keep getting touched
/// across changesets. High counts mean task boundaries keep crossing the
/// same file (split it, or give it an owner) — the registry-level early
/// warning against megafile growth.
pub fn hotspots(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let top = flag_value(args, "--top").unwrap_or(20);
    let min = flag_value(args, "--min").unwrap_or(2);

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(reg) = crate::reg::open_synced_if_present(&cwd) else {
        return ch_error(
            "ch.hotspots",
            "no_registry",
            "no .ivk/ here — run `ivk init` (or any `ivk new`) first",
            json || agent,
        );
    };
    let rows: Vec<HotspotRow> = match reg.hotspots(min, top) {
        Ok(v) => v
            .into_iter()
            .map(|h| HotspotRow {
                path: h.path,
                changeset_count: h.changeset_count,
                workspace_count: h.workspace_count,
            })
            .collect(),
        Err(e) => {
            return ch_error(
                "ch.hotspots",
                "registry_error",
                &format!("{}", e),
                json || agent,
            )
        }
    };

    if json || agent {
        let steps = if agent {
            Some(if rows.is_empty() {
                vec![format!(
                    "No path is touched by {} or more changesets — no contention hotspots.",
                    min
                )]
            } else {
                vec![
                    format!(
                        "{} hotspot path(s); hottest: {} ({} changesets from {} workspace(s)).",
                        rows.len(),
                        rows[0].path,
                        rows[0].changeset_count,
                        rows[0].workspace_count
                    ),
                    "Repeatedly-touched files are where conflicts concentrate: consider splitting them into smaller modules or routing all tasks that touch them through one workspace.".into(),
                ]
            })
        } else {
            None
        };
        let env = Envelope {
            ok: true,
            command: "ch.hotspots",
            next_command: Some("ivk status --agent --json".into()),
            recommended_next_steps: steps,
            error: None,
            data: HotspotsPayload {
                count: rows.len(),
                min_changesets: min,
                hotspots: rows,
            },
        };
        print_json(&env);
    } else if rows.is_empty() {
        println!("no hotspots (no path touched by >= {} changesets).", min);
    } else {
        println!("{:<48} {:>10} {:>11}", "path", "changesets", "workspaces");
        for r in &rows {
            println!(
                "{:<48} {:>10} {:>11}",
                r.path, r.changeset_count, r.workspace_count
            );
        }
    }
    0
}

/// Parse `--flag N` from the arg list.
fn flag_value(args: &[&str], flag: &str) -> Option<u32> {
    args.iter()
        .position(|a| *a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

/// `ivk ch check <id> [<target-rev>]` — does the changeset merge cleanly
/// onto `target-rev` (default `HEAD` of the source repo)? A pure
/// object-store check via merge-tree: no working tree, no workspace, no
/// side effects beyond recording the fact in the registry. Exit 0 for both
/// verdicts — a conflict is a successful check, not an error.
pub fn check(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let positionals: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();
    let (id, target_ref) = match positionals.as_slice() {
        [id, target] => (*id, *target),
        [id] => (*id, "HEAD"),
        _ => {
            return ch_error(
                "ch.check",
                "missing_argument",
                "ch check requires a changeset id (and optionally a target rev, default HEAD)",
                json || agent,
            )
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let c = match load_changeset(&cwd, id) {
        Ok(c) => c,
        Err(LoadError::NotFound) => {
            return ch_error(
                "ch.check",
                "not_found",
                &format!("no changeset `{}`", id),
                json || agent,
            )
        }
        Err(LoadError::BadMetadata(e)) => {
            return ch_error(
                "ch.check",
                "bad_metadata",
                &format!("invalid changeset metadata: {}", e),
                json || agent,
            )
        }
    };

    let git = GitCliBackend::new();
    let target_snapshot = match git.resolve_revision(&cwd, target_ref) {
        Ok(s) => s,
        Err(_) => {
            return ch_error(
                "ch.check",
                "git_rev_parse_failed",
                &format!("could not resolve target rev `{}`", target_ref),
                json || agent,
            )
        }
    };

    let merge = match git.merge_check(&cwd, &c.base_snapshot, &target_snapshot, &c.result_snapshot)
    {
        Ok(m) => m,
        Err(e) => {
            return ch_error(
                "ch.check",
                "git_merge_tree_failed",
                &format!("{}", e),
                json || agent,
            )
        }
    };

    // Record the fact; the check is still useful without a registry.
    if let Some(reg) = crate::reg::open_synced(&cwd) {
        let checked_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = reg.record_check(&ivk_core::ChangesetCheckRecord {
            changeset_id: c.id.clone(),
            target_ref: target_ref.to_string(),
            target_snapshot: target_snapshot.clone(),
            clean: merge.clean,
            conflict_paths: merge.conflict_paths.clone(),
            checked_at_unix: checked_at,
        });
    }

    let target_short = &target_snapshot[..12.min(target_snapshot.len())];
    let next = if merge.clean {
        format!("ivk export {} agent/{}", c.id, c.workspace_name)
    } else {
        format!("ivk ch show {}", c.id)
    };
    if json || agent {
        let env = Envelope {
            ok: true,
            command: "ch.check",
            next_command: Some(next),
            recommended_next_steps: if agent {
                Some(if merge.clean {
                    vec![
                        format!(
                            "Changeset {} merges cleanly onto {} ({}).",
                            c.id, target_ref, target_short
                        ),
                        format!(
                            "Safe to export: `ivk export {} agent/{}`.",
                            c.id, c.workspace_name
                        ),
                    ]
                } else {
                    vec![
                        format!(
                            "Changeset {} conflicts with {} ({}) at {} path(s): {}.",
                            c.id,
                            target_ref,
                            target_short,
                            merge.conflict_paths.len(),
                            merge.conflict_paths.join(", ")
                        ),
                        format!(
                            "If workspace `{}` still exists: `git -C .ivk/workspaces/{} rebase {}`, resolve, then `ivk ch new {}` and re-check.",
                            c.workspace_name, c.workspace_name, target_ref, c.workspace_name
                        ),
                    ]
                })
            } else {
                None
            },
            error: None,
            data: ChCheckPayload {
                changeset_id: c.id.clone(),
                target_ref: target_ref.to_string(),
                target_snapshot: target_snapshot.clone(),
                clean: merge.clean,
                conflict_paths: merge.conflict_paths,
                merged_tree: merge.merged_tree,
            },
        };
        print_json(&env);
    } else if merge.clean {
        println!(
            "check {}: clean against {} ({})",
            c.id, target_ref, target_short
        );
        println!("  next: {}", next);
    } else {
        println!(
            "check {}: CONFLICT with {} ({}) — {} path(s):",
            c.id,
            target_ref,
            target_short,
            merge.conflict_paths.len()
        );
        for p in &merge.conflict_paths {
            println!("    {}", p);
        }
        println!(
            "  hint: rebase workspace `{}` onto {} and run `ivk ch new {}` again",
            c.workspace_name, target_ref, c.workspace_name
        );
    }
    0
}

pub fn export(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let positionals: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();
    let (id, branch_arg) = match positionals.as_slice() {
        [id, branch] => (*id, Some(*branch)),
        [id] => (*id, None),
        _ => {
            return ch_error(
                "export",
                "missing_argument",
                "export requires a changeset id (and optionally a branch name)",
                json || agent,
            )
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let c = match load_changeset(&cwd, id) {
        Ok(c) => c,
        Err(LoadError::NotFound) => {
            return ch_error(
                "export",
                "not_found",
                &format!("no changeset `{}`", id),
                json || agent,
            )
        }
        Err(LoadError::BadMetadata(e)) => {
            return ch_error(
                "export",
                "bad_metadata",
                &format!("invalid changeset metadata: {}", e),
                json || agent,
            )
        }
    };

    let branch = branch_arg
        .map(String::from)
        .unwrap_or_else(|| format!("agent/{}", c.workspace_name));

    // Create or update the branch ref in the source repo.
    if GitCliBackend::new()
        .create_branch(&cwd, &branch, &c.result_snapshot, true)
        .is_err()
    {
        return ch_error(
            "export",
            "git_branch_failed",
            &format!("git branch --force {} {} failed", branch, c.result_snapshot),
            json || agent,
        );
    }

    // Stamp the export so `ch show` / future selectors can see it.
    if let Some(reg) = crate::reg::open_synced(&cwd) {
        let _ = reg.mark_exported(&c.id, &branch);
    }

    if json || agent {
        let env = Envelope {
            ok: true,
            command: "export",
            next_command: Some(format!("git push origin {}", branch)),
            recommended_next_steps: if agent {
                Some(vec![
                    format!(
                        "Branch `{}` now points at {}.",
                        branch,
                        &c.result_snapshot[..12]
                    ),
                    format!(
                        "Push and open a PR: `git push origin {} && gh pr create`.",
                        branch
                    ),
                ])
            } else {
                None
            },
            error: None,
            data: ExportPayload {
                changeset_id: c.id.clone(),
                branch: branch.clone(),
                sha: c.result_snapshot.clone(),
            },
        };
        print_json(&env);
    } else {
        println!(
            "exported {} -> branch {} (sha {})",
            c.id,
            branch,
            &c.result_snapshot[..12]
        );
        println!("  next: git push origin {}", branch);
    }
    0
}

pub fn patch(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let positionals: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();
    let (id, out_arg) = match positionals.as_slice() {
        [id, out] => (*id, Some(*out)),
        [id] => (*id, None),
        _ => {
            return ch_error(
                "patch",
                "missing_argument",
                "patch requires a changeset id (and optionally an output path)",
                json || agent,
            )
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let c = match load_changeset(&cwd, id) {
        Ok(c) => c,
        Err(LoadError::NotFound) => {
            return ch_error(
                "patch",
                "not_found",
                &format!("no changeset `{}`", id),
                json || agent,
            )
        }
        Err(LoadError::BadMetadata(e)) => {
            return ch_error(
                "patch",
                "bad_metadata",
                &format!("invalid changeset metadata: {}", e),
                json || agent,
            )
        }
    };

    // Generate a unified diff between base..result snapshots.
    let out = match GitCliBackend::new().diff_patch(
        &cwd,
        DiffTarget::CommitRange {
            base: &c.base_snapshot,
            head: &c.result_snapshot,
        },
        true,
    ) {
        Ok(bytes) => bytes,
        Err(e) => {
            return ch_error(
                "patch",
                "git_diff_failed",
                &format!("git diff failed: {}", e),
                json || agent,
            )
        }
    };

    let out_path: PathBuf = match out_arg {
        Some(p) => PathBuf::from(p),
        None => cwd.join("patches").join(format!("{}.patch", id)),
    };
    if let Some(parent) = out_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return ch_error(
                "patch",
                "io_error",
                &format!("could not create {}: {}", parent.display(), e),
                json || agent,
            );
        }
    }
    let bytes_written = out.len() as u64;
    if let Err(e) = fs::write(&out_path, &out) {
        return ch_error(
            "patch",
            "io_error",
            &format!("could not write {}: {}", out_path.display(), e),
            json || agent,
        );
    }

    if json || agent {
        let env = Envelope {
            ok: true,
            command: "patch",
            next_command: Some(format!("git apply {}", out_path.display())),
            recommended_next_steps: if agent {
                Some(vec![
                    format!(
                        "Patch written to {} ({} bytes).",
                        out_path.display(),
                        bytes_written
                    ),
                    format!("Apply elsewhere with `git apply {}`.", out_path.display()),
                ])
            } else {
                None
            },
            error: None,
            data: PatchPayload {
                changeset_id: c.id.clone(),
                output_path: out_path.display().to_string(),
                bytes_written,
            },
        };
        print_json(&env);
    } else {
        println!(
            "wrote patch {} -> {} ({} bytes)",
            c.id,
            out_path.display(),
            bytes_written
        );
    }
    0
}

fn ch_error(command: &'static str, code: &'static str, msg: &str, as_json: bool) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command,
            next_command: Some("ivk help".into()),
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
    1
}

fn positional<'a>(args: &'a [&'a str]) -> Option<&'a str> {
    args.iter().copied().find(|a| !a.starts_with('-'))
}

fn diff_stat_between(cwd: &Path, base: &str, head: &str) -> (u32, u32, u32) {
    GitCliBackend::new()
        .diff_stat(cwd, DiffTarget::CommitRange { base, head })
        .map(|d| (d.files_changed, d.insertions, d.deletions))
        .unwrap_or((0, 0, 0))
}
