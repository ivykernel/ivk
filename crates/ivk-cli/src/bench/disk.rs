//! `ivk bench disk` — materialize N workspaces and report the disk-accounting
//! triad: apparent (du -A), allocated (per-file blocks), real (df delta). The
//! triad makes the APFS-clonefile sharing visible: apparent ≈ N·repo_size,
//! allocated under-reports, real is the ground truth.

use serde::Serialize;

use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

use super::harness::{
    df_free_kb, dir_allocated_bytes, dir_apparent_bytes, human_bytes, prepare, BenchDir, EnvBlock,
};

#[derive(Serialize)]
struct DiskPayload {
    params: Params,
    env: EnvBlock,
    repo: Repo,
    workspaces: Workspaces,
    ratios: Ratios,
    notes: Vec<&'static str>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct Params {
    count: usize,
    from: &'static str,
    from_sha: String,
}

#[derive(Serialize)]
struct Repo {
    apparent_kb: u64,
    path: String,
}

#[derive(Serialize)]
struct Workspaces {
    count: usize,
    apparent_kb: u64,
    actual_kb_du_blocks: u64,
    real_kb_df_delta: u64,
}

#[derive(Serialize)]
struct Ratios {
    savings_ratio: f64,
    disk_per_workspace_real_kb: f64,
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

    let prelude = match prepare("disk", "b") {
        Ok(p) => p,
        Err(e) => return error(e.code, &e.message, &e.next_command, json || agent),
    };
    let guard = BenchDir::new(prelude.bench_root.clone(), prelude.cwd.clone());

    let mut created = 0usize;
    let mut first_failure: Option<String> = None;
    for i in 0..count {
        let dst = prelude.bench_root.join(format!("ws-{:04}", i));
        match ivk_core::materialize_workspace(&ivk_core::MaterializeOptions {
            src: prelude.cwd.clone(),
            dst,
            with_git: true,
        }) {
            Ok(_) => created += 1,
            Err(e) => {
                if first_failure.is_none() {
                    first_failure = Some(format!("ws-{:04}: {}", i, e));
                }
            }
        }
    }

    if created == 0 {
        // Bench is meaningless. Surface as an error envelope, drop guard so the
        // empty bench dir is removed.
        let msg = first_failure.unwrap_or_else(|| "no workspaces materialized".into());
        drop(guard);
        return error(
            "materialize_failed",
            &format!("every workspace failed to materialize: {}", msg),
            "ivk doctor",
            json || agent,
        );
    }

    let apparent_kb = dir_apparent_bytes(&prelude.bench_root) / 1024;
    let actual_kb = dir_allocated_bytes(&prelude.bench_root) / 1024;
    let after = df_free_kb(&prelude.cwd).unwrap_or(0);
    let real_kb = prelude.df_free_before_kb.saturating_sub(after);

    // Estimate the source repo size for the savings_ratio. Excludes .ivk/bench
    // so the bench workspaces don't double-count.
    let repo_apparent_bytes = repo_apparent_excluding_bench(&prelude.cwd);
    let repo_apparent_kb = repo_apparent_bytes / 1024;
    let expected_kb = repo_apparent_kb.saturating_mul(count as u64);
    let savings_ratio = if real_kb == 0 {
        f64::INFINITY
    } else {
        expected_kb as f64 / real_kb as f64
    };
    let per_ws = if count == 0 {
        0.0
    } else {
        real_kb as f64 / count as f64
    };

    let mut warnings: Vec<String> = Vec::new();
    if created != count {
        warnings.push(format!(
            "{} of {} workspaces failed to materialize",
            count - created,
            count
        ));
    }
    if real_kb == 0 {
        warnings.push("df reported zero free-space delta; savings_ratio is undefined".into());
    }

    let lp_blurb = if real_kb == 0 {
        format!(
            "{} workspaces of a {} repo cost ~0 bytes on disk (df reported zero delta).",
            created,
            human_bytes(repo_apparent_kb * 1024)
        )
    } else {
        format!(
            "{} workspaces of a {} repo cost {} on disk — {}x scale at {} cost.",
            created,
            human_bytes(repo_apparent_kb * 1024),
            human_bytes(real_kb * 1024),
            created,
            fmt_scale(savings_ratio),
        )
    };

    let payload = DiskPayload {
        params: Params {
            count,
            from: "HEAD",
            from_sha: prelude.from_sha,
        },
        env: prelude.env,
        repo: Repo {
            apparent_kb: repo_apparent_kb,
            path: prelude.cwd.display().to_string(),
        },
        workspaces: Workspaces {
            count: created,
            apparent_kb,
            actual_kb_du_blocks: actual_kb,
            real_kb_df_delta: real_kb,
        },
        ratios: Ratios {
            savings_ratio: if savings_ratio.is_finite() { savings_ratio } else { 0.0 },
            disk_per_workspace_real_kb: per_ws,
            lp_blurb,
        },
        notes: vec![
            "apparent_kb is the logical file-size sum (du -A equivalent).",
            "actual_kb_du_blocks (MetadataExt::blocks()*512) under-reports on APFS clonefile / Linux reflink because shared extents are credited per file.",
            "real_kb_df_delta from `df -k` before/after is ground truth on APFS; can be inflated by concurrent disk activity (Spotlight, backups).",
        ],
        warnings,
    };

    emit(payload, count, json, agent);
    drop(guard);
    0
}

fn fmt_scale(r: f64) -> String {
    if !r.is_finite() {
        "∞ (df reported zero delta)".into()
    } else if r >= 100.0 {
        format!("1/{:.0}x", r)
    } else if r >= 1.0 {
        format!("1/{:.2}x", r)
    } else {
        format!("{:.2}x", r)
    }
}

fn repo_apparent_excluding_bench(cwd: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    let entries = match std::fs::read_dir(cwd) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == ".ivk" {
            continue;
        }
        let md = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if md.file_type().is_symlink() {
            continue;
        }
        if md.is_dir() {
            total = total.saturating_add(dir_apparent_bytes(&path));
        } else {
            total = total.saturating_add(md.len());
        }
    }
    total
}

fn emit(p: DiskPayload, count: usize, json: bool, agent: bool) {
    let next = format!("ivk bench gc --count {}", count);
    if json || agent {
        let steps = if agent {
            Some(vec![
                format!(
                    "{} workspaces, df-delta {}.",
                    p.workspaces.count,
                    human_bytes(p.workspaces.real_kb_df_delta * 1024),
                ),
                "Paste `ratios.lp_blurb` into the LP.".into(),
            ])
        } else {
            None
        };
        let env = Envelope {
            ok: true,
            command: "bench.disk",
            next_command: Some(next),
            recommended_next_steps: steps,
            error: None,
            data: p,
        };
        print_json(&env);
    } else {
        println!("{}", p.ratios.lp_blurb);
    }
}

fn error(code: &'static str, msg: &str, next: &str, as_json: bool) -> i32 {
    if as_json {
        let env: Envelope<()> = Envelope {
            ok: false,
            command: "bench.disk",
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
