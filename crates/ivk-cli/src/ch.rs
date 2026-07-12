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

    // Pull a shortstat for the response.
    let (files_changed, insertions, deletions) =
        diff_stat_between(&cwd, &changeset.base_snapshot, &changeset.result_snapshot);

    if json || agent {
        let env = Envelope {
            ok: true,
            command: "ch.new",
            next_command: Some(format!("ivk export {} agent/{}", id, name)),
            recommended_next_steps: if agent {
                Some(vec![
                    format!("Changeset {} created from workspace {}.", id, name),
                    format!(
                        "Export to a Git branch: `ivk export {} agent/{}`.",
                        id, name
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
        println!("  next: ivk export {} agent/{}", id, name);
    }
    0
}

pub fn ch_ls(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ch_dir = cwd.join(".ivk").join("changesets");

    let mut changesets: Vec<Changeset> = Vec::new();
    if let Ok(entries) = fs::read_dir(&ch_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(s) = fs::read_to_string(&p) {
                if let Ok(c) = serde_json::from_str::<Changeset>(&s) {
                    changesets.push(c);
                }
            }
        }
    }
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
    let path = cwd
        .join(".ivk")
        .join("changesets")
        .join(format!("{}.json", id));
    let body = match fs::read_to_string(&path) {
        Ok(b) => b,
        Err(_) => {
            return ch_error(
                "ch.show",
                "not_found",
                &format!("no changeset `{}`", id),
                json || agent,
            )
        }
    };
    let c: Changeset = match serde_json::from_str(&body) {
        Ok(c) => c,
        Err(e) => {
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
    let path = cwd
        .join(".ivk")
        .join("changesets")
        .join(format!("{}.json", id));
    let body = match fs::read_to_string(&path) {
        Ok(b) => b,
        Err(_) => {
            return ch_error(
                "export",
                "not_found",
                &format!("no changeset `{}`", id),
                json || agent,
            )
        }
    };
    let c: Changeset = match serde_json::from_str(&body) {
        Ok(c) => c,
        Err(e) => {
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
    let meta_path = cwd
        .join(".ivk")
        .join("changesets")
        .join(format!("{}.json", id));
    let body = match fs::read_to_string(&meta_path) {
        Ok(b) => b,
        Err(_) => {
            return ch_error(
                "patch",
                "not_found",
                &format!("no changeset `{}`", id),
                json || agent,
            )
        }
    };
    let c: Changeset = match serde_json::from_str(&body) {
        Ok(c) => c,
        Err(e) => {
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
