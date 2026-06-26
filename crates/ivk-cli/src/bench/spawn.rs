//! `ivk bench spawn` — measure ivk's own workspace materialization on the cwd
//! repo. Materializes N workspaces into a temp bench dir, records per-workspace
//! microseconds, then drops the BenchDir guard so cleanup happens even on
//! panic.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

use super::harness::{
    df_free_kb, dir_apparent_bytes, human_bytes, human_ms, prepare, BenchDir, EnvBlock, Stats,
};

#[derive(Serialize)]
struct SpawnPayload {
    params: Params,
    env: EnvBlock,
    timings_ms: Timings,
    disk: Disk,
    workspaces: Counts,
    run_dir: String,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct Params {
    count: usize,
    from: &'static str,
    from_sha: String,
    prefix: &'static str,
}

#[derive(Serialize)]
struct Timings {
    total_wall_ms: f64,
    per_workspace: Stats,
}

#[derive(Serialize)]
struct Disk {
    apparent_kb: u64,
    real_kb: u64,
}

#[derive(Serialize)]
struct Counts {
    created: usize,
    failed: usize,
    first_failure_reason: Option<String>,
}

pub fn run(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let count = parse_count(args).unwrap_or(10);
    if !(1..=10_000).contains(&count) {
        return error(
            "usage_error",
            "--count must be between 1 and 10000",
            "ivk bench help",
            json || agent,
        );
    }

    let prelude = match prepare("spawn", "b") {
        Ok(p) => p,
        Err(e) => return error(e.code, &e.message, &e.next_command, json || agent),
    };
    let guard = BenchDir::new(prelude.bench_root.clone(), prelude.cwd.clone());

    let mut elapsed_us: Vec<u128> = Vec::with_capacity(count);
    let mut first_failure: Option<String> = None;
    let mut created = 0usize;
    let wall_t0 = Instant::now();
    for i in 0..count {
        let dst = prelude.bench_root.join(format!("ws-{:04}", i));
        let t0 = Instant::now();
        let res = ivk_core::materialize_workspace(&ivk_core::MaterializeOptions {
            src: prelude.cwd.clone(),
            dst: dst.clone(),
            with_git: true,
        });
        let elapsed = t0.elapsed().as_micros();
        match res {
            Ok(_) => {
                elapsed_us.push(elapsed);
                created += 1;
            }
            Err(e) => {
                if first_failure.is_none() {
                    first_failure = Some(format!("ws-{:04}: {}", i, e));
                }
            }
        }
    }
    let total_wall_ms = wall_t0.elapsed().as_secs_f64() * 1000.0;

    let apparent_kb = dir_apparent_bytes(&prelude.bench_root) / 1024;
    let real_kb = df_real_kb(&prelude.cwd, prelude.df_free_before_kb);

    let mut warnings: Vec<String> = Vec::new();
    if count == 1 {
        warnings.push("sample size n=1; per-workspace stats not meaningful".into());
    }
    if created == 0 {
        warnings.push("zero workspaces materialized; timings and disk are meaningless".into());
    }

    let payload = SpawnPayload {
        params: Params {
            count,
            from: "HEAD",
            from_sha: prelude.from_sha,
            prefix: "b",
        },
        env: prelude.env,
        timings_ms: Timings {
            total_wall_ms,
            per_workspace: Stats::from_micros(elapsed_us),
        },
        disk: Disk {
            apparent_kb,
            real_kb,
        },
        workspaces: Counts {
            created,
            failed: count - created,
            first_failure_reason: first_failure,
        },
        run_dir: prelude.bench_root.display().to_string(),
        warnings,
    };

    emit(payload, json, agent);
    // Drop guard cleans up the bench dir.
    drop(guard);
    0
}

fn emit(p: SpawnPayload, json: bool, agent: bool) {
    let next = format!("ivk bench compare-git-worktree --count {}", p.params.count);
    if json || agent {
        let steps = if agent {
            Some(vec![
                format!(
                    "Materialized {} workspaces in {} (p50 {} per ws).",
                    p.workspaces.created,
                    human_ms(p.timings_ms.total_wall_ms),
                    human_ms(p.timings_ms.per_workspace.p50_ms)
                ),
                format!(
                    "Apparent {} on disk; real free-space delta {}.",
                    human_bytes(p.disk.apparent_kb * 1024),
                    human_bytes(p.disk.real_kb * 1024)
                ),
                "Run `ivk bench compare-git-worktree` for LP-ready numbers.".into(),
            ])
        } else {
            None
        };
        let env = Envelope {
            ok: true,
            command: "bench.spawn",
            next_command: Some(next),
            recommended_next_steps: steps,
            error: None,
            data: p,
        };
        print_json(&env);
    } else {
        println!(
            "spawn count={} total={} p50={}",
            p.params.count,
            human_ms(p.timings_ms.total_wall_ms),
            human_ms(p.timings_ms.per_workspace.p50_ms)
        );
    }
}

fn error(code: &'static str, msg: &str, next: &str, as_json: bool) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command: "bench.spawn",
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

fn df_real_kb(cwd: &Path, before_kb: u64) -> u64 {
    let after = df_free_kb(cwd).unwrap_or(0);
    before_kb.saturating_sub(after)
}

fn parse_count(args: &[&str]) -> Option<usize> {
    for (i, a) in args.iter().enumerate() {
        if *a == "--count" {
            return args.get(i + 1).and_then(|v| v.parse().ok());
        }
        if let Some(v) = a.strip_prefix("--count=") {
            return v.parse().ok();
        }
    }
    None
}
