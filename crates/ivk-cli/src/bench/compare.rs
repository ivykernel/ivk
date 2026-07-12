//! `ivk bench compare-git-worktree` — run ivk's spawn loop AND a `git worktree
//! add` loop on the same repo, in a randomized order, and report the speedup +
//! disk ratio that the LP wants to show.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime};

use serde::Serialize;

use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

use super::harness::{
    df_free_kb, dir_apparent_bytes, human_bytes, human_ms, prepare, BenchDir, EnvBlock, Stats,
};

#[derive(Serialize)]
struct ComparePayload {
    params: Params,
    env: EnvBlock,
    arms: Arms,
    comparison: Comparison,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct Params {
    count: usize,
    from: &'static str,
    from_sha: String,
    execution_order: Vec<&'static str>,
}

#[derive(Serialize)]
struct Arms {
    ivk: Arm,
    git_worktree: Arm,
}

#[derive(Serialize)]
struct Arm {
    total_wall_ms: f64,
    per_workspace_ms: Stats,
    apparent_kb: u64,
    real_kb: u64,
    errors: usize,
    strategy: &'static str,
}

#[derive(Serialize)]
struct Comparison {
    speedup_total: f64,
    speedup_p50: f64,
    speedup_p99: f64,
    disk_ratio_apparent: f64,
    disk_ratio_real: f64,
    lp_blurb: String,
}

pub fn run(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let count = super::parse_count(args).unwrap_or(10);
    if !(1..=10_000).contains(&count) {
        return error(
            "usage_error",
            "--count must be between 1 and 10000",
            "ivk bench help",
            json || agent,
        );
    }

    let prelude = match prepare("compare-ivk", "b") {
        Ok(p) => p,
        Err(e) => return error(e.code, &e.message, &e.next_command, json || agent),
    };
    let prelude_git = match prepare("compare-git", "b") {
        Ok(p) => p,
        Err(e) => return error(e.code, &e.message, &e.next_command, json || agent),
    };
    let guard_ivk = BenchDir::new(prelude.bench_root.clone(), prelude.cwd.clone());
    let guard_git = BenchDir::new(prelude_git.bench_root.clone(), prelude.cwd.clone());

    // Execution order: XOR-shift one step on (pid ^ ts). Records into payload.
    let seed_bits = (std::process::id() as u128)
        ^ SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
    let order: Vec<&'static str> = if seed_bits & 1 == 0 {
        vec!["ivk", "git_worktree"]
    } else {
        vec!["git_worktree", "ivk"]
    };

    let mut arm_ivk: Option<Arm> = None;
    let mut arm_git: Option<Arm> = None;

    for which in &order {
        match *which {
            "ivk" => {
                arm_ivk = Some(run_ivk_arm(
                    &prelude.cwd,
                    &prelude.bench_root,
                    count,
                    prelude.df_free_before_kb,
                ));
            }
            "git_worktree" => {
                arm_git = Some(run_git_arm(
                    &prelude.cwd,
                    &prelude_git.bench_root,
                    count,
                    &prelude.from_sha,
                    prelude_git.df_free_before_kb,
                ));
            }
            _ => {}
        }
    }
    let ivk_arm = arm_ivk.expect("arm scheduled");
    let git_arm = arm_git.expect("arm scheduled");

    let speedup_total = ratio(git_arm.total_wall_ms, ivk_arm.total_wall_ms);
    let speedup_p50 = ratio(
        git_arm.per_workspace_ms.p50_ms,
        ivk_arm.per_workspace_ms.p50_ms,
    );
    let speedup_p99 = ratio(
        git_arm.per_workspace_ms.p99_ms,
        ivk_arm.per_workspace_ms.p99_ms,
    );
    let disk_ratio_apparent = ratio_disk(ivk_arm.apparent_kb, git_arm.apparent_kb);
    let disk_ratio_real = ratio_disk(ivk_arm.real_kb, git_arm.real_kb);

    let mut warnings: Vec<String> = Vec::new();
    if count == 1 {
        warnings.push("sample size n=1; per-workspace stats not meaningful".into());
    }
    if !ivk_arm.total_wall_ms.is_finite() || ivk_arm.total_wall_ms == 0.0 {
        warnings.push("ivk arm wall time rounded to 0 ms; speedups treated as ≥1.0x".into());
    }

    let lp_blurb = format!(
        "ivk spawned {} workspaces {} faster than git worktree ({} vs {}) using {} the disk ({} vs {}).",
        count,
        fmt_speedup(speedup_total),
        human_ms(ivk_arm.total_wall_ms),
        human_ms(git_arm.total_wall_ms),
        fmt_ratio_pct(disk_ratio_real),
        human_bytes(ivk_arm.real_kb * 1024),
        human_bytes(git_arm.real_kb * 1024),
    );

    let payload = ComparePayload {
        params: Params {
            count,
            from: "HEAD",
            from_sha: prelude.from_sha,
            execution_order: order,
        },
        env: prelude.env,
        arms: Arms {
            ivk: ivk_arm,
            git_worktree: git_arm,
        },
        comparison: Comparison {
            speedup_total: finite_or(speedup_total, 0.0),
            speedup_p50: finite_or(speedup_p50, 0.0),
            speedup_p99: finite_or(speedup_p99, 0.0),
            disk_ratio_apparent: finite_or(disk_ratio_apparent, 0.0),
            disk_ratio_real: finite_or(disk_ratio_real, 0.0),
            lp_blurb,
        },
        warnings,
    };

    emit(payload, count, json, agent);
    drop(guard_ivk);
    drop(guard_git);
    0
}

fn run_ivk_arm(cwd: &Path, root: &Path, count: usize, df_before: u64) -> Arm {
    let mut us: Vec<u128> = Vec::with_capacity(count);
    let mut errors = 0usize;
    let wall = Instant::now();
    for i in 0..count {
        let dst = root.join(format!("ws-{:04}", i));
        let t0 = Instant::now();
        let r = ivk_core::materialize_workspace(&ivk_core::MaterializeOptions {
            src: cwd.to_path_buf(),
            dst,
            with_git: true,
            rev: None,
        });
        let el = t0.elapsed().as_micros();
        match r {
            Ok(_) => us.push(el),
            Err(_) => errors += 1,
        }
    }
    let total_wall_ms = wall.elapsed().as_secs_f64() * 1000.0;
    let apparent_kb = dir_apparent_bytes(root) / 1024;
    let after = df_free_kb(cwd).unwrap_or(0);
    let real_kb = df_before.saturating_sub(after);
    Arm {
        total_wall_ms,
        per_workspace_ms: Stats::from_micros(us),
        apparent_kb,
        real_kb,
        errors,
        strategy: super::harness_strategy(),
    }
}

fn run_git_arm(cwd: &Path, root: &Path, count: usize, sha: &str, df_before: u64) -> Arm {
    let mut us: Vec<u128> = Vec::with_capacity(count);
    let mut errors = 0usize;
    let wall = Instant::now();
    for i in 0..count {
        let dst = root.join(format!("ws-{:04}", i));
        let t0 = Instant::now();
        let out = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["worktree", "add", "-q", "--detach"])
            .arg(&dst)
            .arg(sha)
            .output();
        let el = t0.elapsed().as_micros();
        match out {
            Ok(o) if o.status.success() => us.push(el),
            _ => errors += 1,
        }
    }
    let total_wall_ms = wall.elapsed().as_secs_f64() * 1000.0;
    let apparent_kb = dir_apparent_bytes(root) / 1024;
    let after = df_free_kb(cwd).unwrap_or(0);
    let real_kb = df_before.saturating_sub(after);
    Arm {
        total_wall_ms,
        per_workspace_ms: Stats::from_micros(us),
        apparent_kb,
        real_kb,
        errors,
        strategy: "git-worktree-add",
    }
}

fn ratio(num: f64, den: f64) -> f64 {
    // Caller treats this as "git/ivk" — when ivk's timing rounded to 0, we
    // can't claim an infinite speedup honestly. Treat as "at least 1x" so the
    // JSON envelope remains valid (INFINITY isn't JSON-representable).
    if !den.is_finite() || den <= 0.0 {
        return if num > 0.0 { num.max(1.0) } else { 1.0 };
    }
    num / den
}

/// Disk usage ratio (ivk / git). When git==0 but ivk>0, ivk is strictly worse
/// — return INFINITY so callers/formatters can decide how to render. When
/// both are 0, treat as parity. Callers must sanitize INFINITY before JSON.
fn ratio_disk(ivk_kb: u64, git_kb: u64) -> f64 {
    match (ivk_kb, git_kb) {
        (0, 0) => 1.0,
        (_, 0) => f64::INFINITY,
        (i, g) => i as f64 / g as f64,
    }
}

/// Sanitize for JSON: returns `fallback` when value is non-finite.
fn finite_or(v: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

fn fmt_ratio_pct(r: f64) -> String {
    if !r.is_finite() {
        return "more than".into();
    }
    if r <= 0.0 {
        return "<1%".into();
    }
    let pct = r * 100.0;
    if pct >= 1.0 {
        format!("{:.1}%", pct)
    } else if pct >= 0.01 {
        format!("{:.4}%", pct)
    } else {
        // Express as 1/Nx of the baseline so agents have a stable shape.
        format!("1/{:.0}x", 1.0 / r)
    }
}

fn fmt_speedup(r: f64) -> String {
    if !r.is_finite() {
        return ">100x".into();
    }
    if r >= 100.0 {
        format!("{:.0}x", r)
    } else if r >= 1.0 {
        format!("{:.1}x", r)
    } else {
        // ivk was slower; surface that honestly.
        format!("{:.2}x", r)
    }
}

fn emit(p: ComparePayload, count: usize, json: bool, agent: bool) {
    let next = format!("ivk bench disk --count {}", count);
    if json || agent {
        let steps = if agent {
            Some(vec![
                format!(
                    "ivk was {} faster (p50 {} vs {}).",
                    fmt_speedup(p.comparison.speedup_total),
                    human_ms(p.arms.ivk.per_workspace_ms.p50_ms),
                    human_ms(p.arms.git_worktree.per_workspace_ms.p50_ms),
                ),
                format!(
                    "ivk used {} the disk ({} vs {}).",
                    fmt_ratio_pct(p.comparison.disk_ratio_real),
                    human_bytes(p.arms.ivk.real_kb * 1024),
                    human_bytes(p.arms.git_worktree.real_kb * 1024),
                ),
                "Paste `comparison.lp_blurb` into the LP.".into(),
            ])
        } else {
            None
        };
        let env = Envelope {
            ok: true,
            command: "bench.compare_git_worktree",
            next_command: Some(next),
            recommended_next_steps: steps,
            error: None,
            data: p,
        };
        print_json(&env);
    } else {
        println!("{}", p.comparison.lp_blurb);
    }
}

fn error(code: &'static str, msg: &str, next: &str, as_json: bool) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command: "bench.compare_git_worktree",
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

// silence unused warning when target_os != macos/linux
#[allow(dead_code)]
fn _suppress(_: PathBuf) {}
