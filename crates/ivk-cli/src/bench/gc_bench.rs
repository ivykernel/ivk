//! `ivk bench gc` — measure gc throughput on a synthetic orphan population.
//!
//! Note this is distinct from `ivk gc`: it materializes its OWN workspaces
//! (prefixed with `gc-bench-<pid>-<ts>-ws-NNNN`) under `.ivk/workspaces/`,
//! breaks the admin half of them to force orphan classification, runs gc, and
//! reports timings + the bytes_reclaimed gc reports. Never touches the user's
//! pre-existing workspaces or their changesets.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Instant, SystemTime};

use serde::Serialize;

use crate::gc;
use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

use super::harness::{env_block, human_bytes, EnvBlock};

#[derive(Serialize)]
struct GcBenchPayload {
    params: Params,
    env: EnvBlock,
    setup: Setup,
    dry_run: DryRun,
    execute: Execute,
    sanity: Sanity,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct Params {
    count: usize,
    forced_prefix: String,
}

#[derive(Serialize)]
struct Setup {
    materialized: usize,
    broken_for_orphan_test: usize,
    materialize_total_ms: f64,
}

#[derive(Serialize)]
struct DryRun {
    predicted_removed_workspaces: usize,
    predicted_bytes_reclaimed: u64,
}

#[derive(Serialize)]
struct Execute {
    gc_total_ms: f64,
    removed_workspaces: usize,
    removed_admin: usize,
    skipped_locked: usize,
    bytes_before: u64,
    bytes_after: u64,
    bytes_reclaimed: u64,
    bytes_reclaimed_human: String,
    ms_per_workspace: f64,
}

#[derive(Serialize)]
struct Sanity {
    preserved_user_workspaces: usize,
    leftover_admin_entries: usize,
}

pub fn run(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let count = super::parse_count(args).unwrap_or(10);
    if !(2..=1_000).contains(&count) {
        return error(
            "usage_error",
            "--count must be between 2 and 1000 for bench gc",
            "ivk bench help",
            json || agent,
        );
    }

    let cwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            return error(
                "io_error",
                &format!("cannot resolve cwd: {}", e),
                "ivk doctor",
                json || agent,
            )
        }
    };
    if !cwd.join(".git").exists() {
        return error(
            "not_a_git_repo",
            "no .git here; run `git init` first",
            "git init",
            json || agent,
        );
    }
    let head_ok = Command::new("git")
        .arg("-C")
        .arg(&cwd)
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !head_ok {
        return error(
            "no_commits",
            "HEAD does not point at a commit",
            "git commit --allow-empty -m bootstrap",
            json || agent,
        );
    }

    let pid = std::process::id();
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let forced_prefix = format!("gc-bench-{}-{}", pid, ts);

    let workspaces_dir = cwd.join(".ivk").join("workspaces");
    if let Err(e) = fs::create_dir_all(&workspaces_dir) {
        return error(
            "io_error",
            &format!("cannot create {}: {}", workspaces_dir.display(), e),
            "ivk doctor",
            json || agent,
        );
    }

    // Snapshot pre-existing names so we can verify nothing was disturbed.
    let pre_existing: Vec<String> = match fs::read_dir(&workspaces_dir) {
        Ok(e) => e
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => vec![],
    };

    let mut bench_names: Vec<String> = Vec::with_capacity(count);
    let mat_t0 = Instant::now();
    for i in 0..count {
        let name = format!("{}-ws-{:04}", forced_prefix, i);
        let dst = workspaces_dir.join(&name);
        if ivk_core::materialize_workspace(&ivk_core::MaterializeOptions {
            src: cwd.clone(),
            dst,
            with_git: true,
            rev: None,
        })
        .is_ok()
        {
            bench_names.push(name);
        }
    }
    let materialize_total_ms = mat_t0.elapsed().as_secs_f64() * 1000.0;

    // Break half: drop the admin entry so the workspace becomes an orphan.
    let break_count = bench_names.len() / 2;
    for name in bench_names.iter().take(break_count) {
        let admin = cwd.join(".git").join("worktrees").join(name);
        let _ = fs::remove_dir_all(&admin);
    }

    // One lock spans dry-run AND live so no other gc / bulk rm can mutate the
    // workspace set between prediction and execution. Without this, a concurrent
    // `ivk gc` could remove half the bench orphans mid-run and the prediction/
    // actual counts diverge silently.
    let lock_path = cwd.join(".ivk").join(".gc.lock");
    let _lock = match gc::GcLock::acquire(&lock_path) {
        Ok(l) => l,
        Err(reason) => return error("gc_in_progress", &reason, "ivk help", json || agent),
    };

    let dry_payload = gc::compute_gc_locked(&cwd, true);
    let predicted_count: usize = dry_payload
        .removed_workspaces
        .iter()
        .filter(|n| n.starts_with(&forced_prefix))
        .count();
    let predicted_bytes = dry_payload.bytes_reclaimed;

    let live_t0 = Instant::now();
    let payload = gc::compute_gc_locked(&cwd, false);
    let gc_total_ms = live_t0.elapsed().as_secs_f64() * 1000.0;

    let removed_in_bench: usize = payload
        .removed_workspaces
        .iter()
        .filter(|n| n.starts_with(&forced_prefix))
        .count();
    let removed_admin_in_bench: usize = payload
        .removed_admin
        .iter()
        .filter(|n| n.starts_with(&forced_prefix))
        .count();
    let skipped_in_bench: usize = payload
        .skipped_locked
        .iter()
        .filter(|s| s.name.starts_with(&forced_prefix))
        .count();
    let ms_per_workspace = if removed_in_bench == 0 {
        0.0
    } else {
        gc_total_ms / removed_in_bench as f64
    };

    // Defensive cleanup: anything in bench_names that survived gc, plus admin
    // entries that might still exist for the live half (gc shouldn't have
    // removed them because the workspace is still live and clean — that's what
    // we expect, so we tear it down manually).
    //
    // The prefix guard is a fail-safe: bench_names is built from forced_prefix,
    // but defending against a future refactor that might widen the loop is
    // cheap insurance against catastrophic user-workspace deletion.
    for name in &bench_names {
        if !name.starts_with(&forced_prefix) {
            continue;
        }
        let ws_path = workspaces_dir.join(name);
        if ws_path.exists() {
            let _ = Command::new("git")
                .arg("-C")
                .arg(&cwd)
                .args(["worktree", "remove", "--force"])
                .arg(&ws_path)
                .output();
            let _ = fs::remove_dir_all(&ws_path);
        }
        let admin = cwd.join(".git").join("worktrees").join(name);
        if admin.exists() {
            let _ = fs::remove_dir_all(&admin);
        }
    }
    let _ = Command::new("git")
        .arg("-C")
        .arg(&cwd)
        .args(["worktree", "prune"])
        .output();

    // Sanity: pre_existing workspaces still present?
    let mut preserved = 0usize;
    for n in &pre_existing {
        if workspaces_dir.join(n).exists() {
            preserved += 1;
        }
    }
    let leftover_admin = count_leftover_admin(&cwd, &forced_prefix);

    let payload_out = GcBenchPayload {
        params: Params {
            count: bench_names.len(),
            forced_prefix: forced_prefix.clone(),
        },
        env: env_block(),
        setup: Setup {
            materialized: bench_names.len(),
            broken_for_orphan_test: break_count,
            materialize_total_ms,
        },
        dry_run: DryRun {
            predicted_removed_workspaces: predicted_count,
            predicted_bytes_reclaimed: predicted_bytes,
        },
        execute: Execute {
            gc_total_ms,
            removed_workspaces: removed_in_bench,
            removed_admin: removed_admin_in_bench,
            skipped_locked: skipped_in_bench,
            bytes_before: payload.bytes_before,
            bytes_after: payload.bytes_after,
            bytes_reclaimed: payload.bytes_reclaimed,
            bytes_reclaimed_human: payload.bytes_reclaimed_human.clone(),
            ms_per_workspace,
        },
        sanity: Sanity {
            preserved_user_workspaces: preserved,
            leftover_admin_entries: leftover_admin,
        },
        warnings: {
            let mut w: Vec<String> = Vec::new();
            if predicted_count != removed_in_bench {
                w.push(format!(
                    "dry-run predicted {} removals; live gc actually removed {} — possible concurrent gc",
                    predicted_count, removed_in_bench
                ));
            }
            if bench_names.len() < count {
                w.push(format!(
                    "only {} of {} workspaces materialized",
                    bench_names.len(),
                    count
                ));
            }
            w
        },
    };

    emit(payload_out, json, agent);
    0
}

fn count_leftover_admin(cwd: &Path, prefix: &str) -> usize {
    let admin_dir = cwd.join(".git").join("worktrees");
    match fs::read_dir(&admin_dir) {
        Ok(e) => e
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with(prefix))
            .count(),
        Err(_) => 0,
    }
}

fn emit(p: GcBenchPayload, json: bool, agent: bool) {
    if json || agent {
        let steps = if agent {
            Some(vec![
                format!(
                    "Materialized {} workspaces, broke {} for orphan test.",
                    p.setup.materialized, p.setup.broken_for_orphan_test
                ),
                format!(
                    "gc reclaimed {} ({:.1} ms/workspace).",
                    human_bytes(p.execute.bytes_reclaimed),
                    p.execute.ms_per_workspace
                ),
            ])
        } else {
            None
        };
        let env = Envelope {
            ok: true,
            command: "bench.gc",
            next_command: Some("ivk doctor".into()),
            recommended_next_steps: steps,
            error: None,
            data: p,
        };
        print_json(&env);
    } else {
        println!(
            "gc reclaimed {} in {:.0} ms ({:.1} ms/ws)",
            human_bytes(p.execute.bytes_reclaimed),
            p.execute.gc_total_ms,
            p.execute.ms_per_workspace,
        );
    }
}

fn error(code: &'static str, msg: &str, next: &str, as_json: bool) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command: "bench.gc",
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
    super::exit_for(code)
}
