//! `ivk help` (with `--agent` for agent-friendly machine-readable form).

use serde::Serialize;

use crate::output::{print_json, wants_agent, wants_json, Envelope};

const HUMAN_HELP: &str = "\
ivk — Ivy Kernel: parallel workspace kernel for AI agents.

Golden path:

  1. Create one or more workspaces from HEAD:
       ivk new attempt-1 attempt-2 attempt-3

  2. cd into a workspace; edit, build, test:
       cd .ivk/workspaces/attempt-1
       # ...

  3. List / show / diff / remove workspaces:
       ivk ws ls
       ivk ws show attempt-1
       ivk ws diff attempt-1
       ivk ws rm   attempt-1

  4. Create a changeset, check it merges, and export to a Git branch:
       ivk ch new attempt-1
       ivk ch check <ch-id>        # conflict status vs HEAD (or: ivk ch check <ch-id> main)
       ivk export <ch-id> agent/<task>
       ivk patch  <ch-id>          # optional: write a .patch file
       # ivk ship attempt-1 (coming) — convenience: ch+export+push+gh pr create

  5. Bulk cleanup / recovery:
       ivk gc                       # prune orphan workspaces / admin entries; report bytes reclaimed
       ivk ws rm --exported --yes   # discard workspaces already preserved on agent/<ws> branches
       ivk ws rm --all --yes        # nuclear: discard every workspace (dirty ones skipped unless --force)
       ivk doctor --repair          # roll back half-created / complete half-removed workspaces after a crash

Run `ivk help --agent` for a machine-readable workflow summary.
Run `ivk doctor --agent --json` to check current state.
";

#[derive(Serialize)]
struct AgentHelp {
    overview: &'static str,
    golden_path: Vec<&'static str>,
    critical_rules: Vec<&'static str>,
    diagnostic_command: &'static str,
}

pub fn run(args: &[&str]) -> i32 {
    if wants_agent(args) || wants_json(args) {
        let payload = AgentHelp {
            overview: "Ivy Kernel materializes parallel git worktrees cheaply using APFS clonefile / Linux reflink. Use `ivk new <name>` to create a workspace; cd into `.ivk/workspaces/<name>` to edit and build there.",
            golden_path: vec![
                "Run `ivk doctor --agent --json` first to discover current state.",
                "Create a workspace per task: `ivk new <task-name>`.",
                "cd .ivk/workspaces/<task-name> and do the work inside.",
                "Tests pass: `ivk ch new <task-name>` to record a changeset.",
                "Tests fail: `ivk ws rm <task-name>` to discard.",
                "Before exporting: `ivk ch check <ch-id>` — clean means safe to export; on conflicts rebase the workspace and re-run `ivk ch new`.",
                "Export with `ivk export <ch-id> agent/<task-name>` — never `git push` from inside a workspace directly.",
                "Bulk cleanup: `ivk gc` reports bytes reclaimed; `ivk ws rm --exported --yes` discards already-preserved workspaces; `ivk ws rm --all --yes` discards everything.",
            ],
            critical_rules: vec![
                "Never edit files in the base repo root once a workspace exists for the task.",
                "Never run `git worktree add` manually — use `ivk new` so ivk can track the lifecycle.",
                "Never delete the `.ivk/` directory.",
                "When unsure, run `ivk doctor --agent --json` and follow `next_command`.",
            ],
            diagnostic_command: "ivk doctor --agent --json",
        };
        let env = Envelope {
            ok: true,
            command: "help.agent",
            next_command: Some("ivk doctor --agent --json".into()),
            recommended_next_steps: Some(vec![
                "Run `ivk doctor --agent --json` to verify environment.".into(),
                "Create a workspace with `ivk new <task-name>`.".into(),
            ]),
            error: None,
            data: payload,
        };
        print_json(&env);
        return 0;
    }

    println!("{}", HUMAN_HELP);
    0
}
