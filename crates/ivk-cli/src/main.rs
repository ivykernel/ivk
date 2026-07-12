//! Ivy Kernel CLI.
//!
//! All commands accept `--json` for structured output. Agent-facing commands
//! also accept `--agent`, which switches to a JSON shape that emphasizes the
//! `next_command` field so calling agents have a clear continuation hint.

mod bench;
mod ch;
mod doctor;
mod gc;
mod help;
mod init;
mod output;
mod reg;
mod status;
mod ws;
mod ws_new;

use std::process::exit;

const USAGE: &str = "\
ivk — Ivy Kernel (workspace kernel for AI agents)

Usage:
  ivk --version
  ivk help [--agent]
  ivk init [--agent-instructions] [--json] [--agent]
  ivk status [--json] [--agent]
  ivk doctor [--agent] [--json] [--repair]
  ivk new <name> [<name>...] [--from <rev>] [--json] [--agent]
  ivk ws new <name> [<name>...] [--from <rev>] [--json] [--agent]
  ivk ls   [--json] [--agent]
  ivk du   [<name>...] [--json] [--agent]
  ivk show <name> [--json] [--agent]
  ivk diff <name> [--json]
  ivk rm   <name> [<name>...] [--json]
  ivk rm   --all      [--yes] [--force] [--dry-run] [--json] [--agent]
  ivk rm   --exported [--yes] [--force] [--dry-run] [--json] [--agent]
  ivk ch new <ws-name> [--json] [--agent]
  ivk ch ls [--json] [--agent]
  ivk ch show <ch-id> [--json] [--agent]
  ivk export <ch-id> [<branch>] [--json] [--agent]
  ivk patch  <ch-id> [<output-path>] [--json] [--agent]
  ivk gc   [--dry-run] [--json] [--agent]
  ivk bench {spawn|compare-git-worktree|disk|gc} [--count N] [--json] [--agent]

Pass --agent to commands that support it for an agent-friendly summary
that includes a `next_command` field. Pass --json for machine output.
";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = raw.iter().map(String::as_str).collect();

    let code = match argv.as_slice() {
        [] | ["-h"] | ["--help"] => {
            println!("{}", USAGE);
            0
        }
        ["help", rest @ ..] => help::run(rest),
        ["--version"] | ["-V"] => {
            println!("ivk {}", env!("CARGO_PKG_VERSION"));
            0
        }
        ["doctor", rest @ ..] => doctor::run(rest),
        ["init", rest @ ..] => init::run(rest),
        ["status", rest @ ..] => status::run(rest),
        ["new", rest @ ..] => ws_new::run(rest),
        ["ws", "new", rest @ ..] => ws_new::run(rest),
        ["ls", rest @ ..] | ["ws", "ls", rest @ ..] => ws::ls(rest),
        ["du", rest @ ..] | ["ws", "du", rest @ ..] => ws::du(rest),
        ["show", rest @ ..] | ["ws", "show", rest @ ..] => ws::show(rest),
        ["diff", rest @ ..] | ["ws", "diff", rest @ ..] => ws::diff(rest),
        ["rm", rest @ ..] | ["ws", "rm", rest @ ..] => ws::rm(rest),
        ["ch", "new", rest @ ..] => ch::ch_new(rest),
        ["ch", "ls", rest @ ..] => ch::ch_ls(rest),
        ["ch", "show", rest @ ..] => ch::ch_show(rest),
        ["export", rest @ ..] => ch::export(rest),
        ["patch", rest @ ..] => ch::patch(rest),
        ["gc", rest @ ..] => gc::run(rest),
        ["bench", rest @ ..] => bench::run(rest),
        _ => {
            eprintln!("ivk: unknown command. See `ivk help`.");
            2
        }
    };
    exit(code);
}
