//! `ivk bench` — phase-5 measurement subcommands.
//!
//! All subcommands:
//!   * Refuse cleanly on no-git / no-HEAD-commit cwds.
//!   * Materialize their workspaces into `.ivk/bench/<run-prefix>/`, never
//!     `.ivk/workspaces/` (exception: `bench gc`, which has to live alongside
//!     real workspaces because the gc machinery only classifies that path).
//!   * Hold a `BenchDir` RAII guard so cleanup runs even on panic.
//!   * Emit a stable JSON envelope shape consumable by the LP without
//!     post-processing.

mod compare;
mod disk;
mod gc_bench;
pub(crate) mod harness;
mod spawn;

use crate::output::{print_json, wants_agent, wants_json, Envelope, ErrorBlock};

const USAGE: &str = "\
ivk bench — measurement subcommands

  ivk bench spawn                   [--count N] [--json] [--agent]
  ivk bench compare-git-worktree    [--count N] [--json] [--agent]
  ivk bench disk                    [--count N] [--json] [--agent]
  ivk bench gc                      [--count N] [--json] [--agent]

Defaults: --count 10. Each run materializes from HEAD into a fresh dir under
.ivk/bench/, then removes everything (including via Drop on panic).

JSON output is the canonical form. Pipe to a file:
  ivk bench compare-git-worktree --count 100 --json > lp-data.json
";

pub fn run(args: &[&str]) -> i32 {
    match args {
        [] | ["-h"] | ["--help"] | ["help"] => {
            println!("{}", USAGE);
            0
        }
        ["spawn", rest @ ..] => spawn::run(rest),
        ["compare-git-worktree", rest @ ..] => compare::run(rest),
        ["disk", rest @ ..] => disk::run(rest),
        ["gc", rest @ ..] => gc_bench::run(rest),
        _ => unknown_subcommand(args),
    }
}

fn unknown_subcommand(args: &[&str]) -> i32 {
    let json = wants_json(args);
    let agent = wants_agent(args);
    let msg =
        "unknown bench subcommand. Try one of: spawn, compare-git-worktree, disk, gc".to_string();
    if json || agent {
        let env: Envelope<()> = Envelope {
            ok: false,
            command: "bench",
            next_command: Some("ivk bench help".into()),
            recommended_next_steps: None,
            error: Some(ErrorBlock {
                code: "usage_error",
                message: msg,
            }),
            data: (),
        };
        print_json(&env);
    } else {
        eprintln!("ivk: {}", msg);
    }
    2
}

pub(crate) fn parse_count(args: &[&str]) -> Option<usize> {
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

pub(crate) fn exit_for(code: &str) -> i32 {
    match code {
        "usage_error" | "not_a_git_repo" | "bad_rev" | "no_commits" => 2,
        _ => 1,
    }
}

pub(crate) fn harness_strategy() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "apfs-clonefile"
    }
    #[cfg(target_os = "linux")]
    {
        "linux-reflink-via-cp"
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "unsupported"
    }
}
