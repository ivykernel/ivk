# Ivy Kernel (`ivk`) MVP-to-Launch Plan v2

## Purpose

This document defines a practical development plan for **Ivy Kernel (`ivk`)**, a parallel workspace kernel for AI agents.

This v2 plan strengthens the **Agent Understanding Layer**.

The goal is not only to build a fast workspace kernel, but also to make the kernel understandable and usable by coding agents that have never seen `ivk` before.

---

## Order of operations (spike-first)

Two spikes gate Phase 0:

1. **Working-tree spike** — [`ivk_benchmark_spike.md`](./ivk_benchmark_spike.md). DONE. Validated 55× disk reduction for clonefile vs git worktree. See [`results/summary.md`](./results/summary.md).
2. **Build-artifact spike** — [`ivk_build_artifact_spike.md`](./ivk_build_artifact_spike.md). NOT STARTED. Must answer whether shared dependency stores + shared build caches keep the disk story intact when each workspace actually *builds*.

```text
Spike 1  working-tree clonefile           (DONE: 55× win, GO)
              │
              ▼
        Rust F prototype + bench           (in progress)
              │
              ▼
Spike 2  build artifact sharing           (next)
              │
              ▼
Phase 0  (informed by both spikes)
```

If spike 2 succeeds: ship the integrated "100 workspaces in a few GB even with builds" story.
If spike 2 fails: ship a narrower pitch that explicitly excludes build outputs and recommend per-toolchain config separately.

Repo: `ivykernel/ivk`. Binary name: `ivk`. GitHub organization `ivykernel` is reserved.

---

## Core positioning

**Ivy Kernel (`ivk`) is a parallel workspace kernel for AI agents.**

Git makes branches cheap.  
Ivy Kernel makes workspaces cheap.

```text
Branches are cheap.
Worktrees are not.
Ivy Kernel makes workspaces cheap too.
```

Strategic comparison:

```text
Git:
  commit / branch / working tree are central.
  Human developers stage and commit changes.
  GitHub provides review and collaboration.

Jujutsu:
  change / revision graph are central.
  The working copy is also a revision.
  Local history editing becomes easier.
  Git compatibility is preserved.

Ivy Kernel:
  workspace / overlay / changeset are central.
  Many agents can work in parallel.
  Workspaces are lightweight and disposable.
  Results can be exported back to Git.
```

Core message:

```text
Jujutsu makes one developer's local history easier.
Ivy Kernel makes many agents' parallel workspaces cheap and manageable.
```

New v2 message:

```text
Jujutsu is change-native.
Ivy Kernel is workspace-native and agent-readable.
```

---

# 1. Should `ivk` be written in Rust?

## Recommendation

Yes. Rust is a strong fit for `ivk`.

`ivk` is a local-first CLI tool that needs to perform filesystem operations, process spawning, structured output, Git integration, and later possibly copy-on-write or overlay-like behavior.

Rust is a good fit because:

```text
single native binary distribution
fast startup
strong typing
safe systems programming
good filesystem/process APIs
excellent CLI ecosystem
easy JSON serialization
good cross-platform release tooling
Homebrew-friendly binary distribution
```

Recommended stack:

```text
CLI:
  clap

Serialization:
  serde
  serde_json
  toml

Error handling:
  anyhow
  thiserror

Logging / tracing:
  tracing
  tracing-subscriber

Filesystem:
  walkdir
  ignore
  fs_extra
  tempfile

Git integration:
  shell out to git first
  consider gix later only if needed

Testing:
  assert_cmd
  predicates
  tempfile
  insta

Benchmarking:
  criterion for microbenchmarks
  custom `ivk bench` for product benchmarks

Release:
  cargo-dist
  GitHub Actions
  Homebrew tap
```

For the first version, shell out to `git` instead of building deep Git internals.

This keeps the MVP small and makes compatibility obvious.

---

# 2. Product architecture

## Kernel responsibilities

Ivy Kernel should stay small.

It should handle:

```text
snapshot
workspace
overlay
materialization
changeset
git export
workspace lifecycle
garbage collection
agent-readable protocol
benchmarking
```

It should not handle:

```text
hosted review UI
comments
team permissions
CI dashboard
issue tracking
agent marketplace
organization management
IvyHub collaboration workflows
```

## `ivk` and IvyHub separation

```text
ivk:
  local kernel / CLI / workspace engine

IvyHub:
  hosted review / collaboration / visibility layer
```

Boundary:

```text
ivk creates facts.
IvyHub helps humans and agents judge those facts.
```

`ivk` produces:

```text
workspace metadata
base snapshot
overlay
changeset
file diff
touched paths
exportable Git branch / patch
benchmark metrics
agent-readable status
```

IvyHub can later add:

```text
review comments
approval
test evidence dashboard
agent logs
risk summaries
team collaboration
GitHub PR integration
```

---

# 3. Repository and project setup

Recommended GitHub organization:

```text
ivykernel
```

Recommended repositories:

```text
ivykernel/ivk             # Rust CLI and kernel
ivykernel/docs            # Documentation / GitHub Pages site
ivykernel/homebrew-tap    # Homebrew tap
ivykernel/ivyhub          # Future hosted review layer
```

Initial repo structure:

```text
ivk/
  Cargo.toml
  README.md
  LICENSE
  CHANGELOG.md

  crates/
    ivk-cli/
    ivk-core/

  docs/
    design.md
    agent-protocol.md
    benchmarks.md
    demo.md

  skills/
    ivk/
      SKILL.md
      cli.md
      workflow.md
      mcp.md
      rules/
        safety.md
        git-compatibility.md
      agents/
        openai.yml
        claude.yml
        cursor.yml

  examples/
    todo-100/

  scripts/
    demo-100-agents.sh
    baseline-git-worktree.sh

  .github/
    workflows/
      ci.yml
      release.yml
      pages.yml
```

---

# 4. MVP feature set

The MVP must prove three technical hypotheses and one agent-understanding hypothesis.

## Hypothesis 1

```text
ivk can create many isolated workspaces faster and cheaper than plain Git worktree workflows.
```

## Hypothesis 2

```text
ivk can track, discard, and garbage-collect failed agent workspaces cleanly.
```

## Hypothesis 3

```text
ivk can convert successful workspace results into Git-compatible changesets.
```

## Hypothesis 4

```text
Coding agents can learn the ivk workflow from repo-local skill files and CLI JSON output.
```

This fourth hypothesis is critical.

Git and GitHub are already known by agents because they are everywhere.  
`ivk` is new, so it must teach agents how to use it.

---

# 5. MVP commands

## Project initialization

```bash
ivk init
ivk init --agent-instructions
```

Creates:

```text
.ivk/
  config.toml
  registry.json
  agent-policy.toml
  workspaces/
  overlays/
  changesets/

AGENTS.md
```

## Agent understanding commands

```bash
ivk help --agent
ivk doctor --agent
ivk doctor --agent --json
ivk status --json
```

## Workspace commands

Canonical form (namespaced, explicit) — for scripting and clarity:

```bash
ivk ws new <name> [<name>...] [--from <base>]
ivk ws ls
ivk ws show <name|id>
ivk ws mount <name|id> [<path>]      # default path: ./workspaces/<name>
ivk ws diff <name|id>
ivk ws rm   <name|id>
```

Short top-level aliases for the common case (single-namespace dispatch):

```bash
ivk new <name> [<name>...]           # alias of: ivk ws new
ivk ls                                # alias of: ivk ws ls
ivk mount <name|id> [<path>]          # alias of: ivk ws mount
ivk diff <name|id>                    # alias of: ivk ws diff
ivk rm   <name|id>                    # alias of: ivk ws rm
```

Design notes:

```text
- Name is positional, not a --name flag. --name was redundant with the slot.
- --from defaults to the current Git HEAD; omit it in the common case.
- Multiple names create multiple workspaces in one call:
    ivk new attempt-1 attempt-2 attempt-3
    ivk new attempt-{1,2,3}          # bash brace expansion equivalent
- mount path defaults to ./workspaces/<name>; pass an explicit path only when overriding.
- A workspace can be referenced by name OR by id (ws_01HXYZ). Names are friendlier;
  ids are stable identifiers used by scripts and JSON outputs.
```

## ChangeSet commands

```bash
ivk ch new <ws-name|ws-id>           # named after a workspace
ivk ch ls
ivk ch show <ch-id>
```

## Git export commands

```bash
ivk export <ch-id> [<branch>]        # short form; branch defaults to agent/<ws-name>
ivk patch  <ch-id> [<output-path>]   # short form; default output: ./patches/<ch-id>.patch

ivk ship   <ws-name|ws-id>           # convenience: ch new + export + git push + gh pr create
                                     # Phase 3+. See Risk 9 in section 15.
```

## Benchmark commands

```bash
ivk bench spawn --count 100 --from main
ivk bench compare-git-worktree --count 100
ivk bench disk
ivk bench gc
```

## Cleanup

```bash
ivk gc
```

---

# 6. Internal data model

## Snapshot

In the MVP:

```text
snapshot_id = Git commit hash
```

No custom object database yet.

## Workspace

```json
{
  "id": "ws_01HXYZ",
  "name": "fix-login",
  "base_snapshot": "abc123",
  "overlay_id": "ov_01HXYZ",
  "status": "created",
  "mount_path": "./workspaces/ws_01HXYZ",
  "created_at": "2026-06-25T00:00:00Z",
  "metadata": {
    "agent": "codex",
    "task_id": "issue-123"
  }
}
```

## Overlay

MVP implementation can be pragmatic.

At first, a mounted workspace may be a physical directory.

The overlay can be derived from Git diff against the base snapshot.

MVP overlay strategy:

```text
1. Create lightweight workspace metadata.
2. Materialize by copying or checking out files into a managed directory.
3. Compute overlay by diffing against base snapshot.
4. Store overlay metadata and file patches.
```

Phase 2 can improve this with:

```text
hardlinks
reflinks
copy-on-write
FUSE
container snapshotter
shared dependency cache
```

## ChangeSet

```json
{
  "id": "ch_01HABC",
  "workspace_id": "ws_01HXYZ",
  "base_snapshot": "abc123",
  "result_snapshot": "def456",
  "touched_paths": [
    "src/auth.ts",
    "tests/auth.test.ts"
  ],
  "diff_summary": {
    "files_changed": 2,
    "insertions": 42,
    "deletions": 10
  },
  "created_at": "2026-06-25T00:00:00Z"
}
```

## Registry

Start with:

```text
.ivk/registry.json
```

Move to SQLite when concurrent writes become real.

Because parallel agents may write concurrently, file locking should be added early if JSON is used.

---

# 7. Agent Understanding Layer

## Why this is core

Git and GitHub are already familiar to coding agents.

Agents often know how to run:

```bash
git status
git checkout -b feature/foo
git add .
git commit -m "..."
gh pr create
```

`ivk` is new.

Without explicit guidance, agents will fall back to Git habits.

That would undermine the whole product.

Therefore, `ivk` must be designed as:

```text
a CLI
+ a local workspace kernel
+ an agent-readable protocol
+ a skill package
```

## Agent-readable assets

`ivk` should ship with:

```text
skills/ivk/SKILL.md
skills/ivk/cli.md
skills/ivk/workflow.md
skills/ivk/mcp.md
skills/ivk/rules/safety.md
skills/ivk/rules/git-compatibility.md
```

These are not just documentation.

They are operational instructions for coding agents.

---

## `skills/ivk/SKILL.md`

This is the main agent-facing entry point.

It should include:

```text
what ivk is
when to use ivk
allowed commands
golden path workflow
critical rules
what not to do
how to recover when unsure
```

Example:

```md
---
name: ivk
description: Manage Ivy Kernel workspaces for parallel AI-agent development.
user-invocable: false
allowed-tools: Bash(ivk *)
---

# Ivy Kernel Skill

Use Ivy Kernel (`ivk`) when working in repositories that have `.ivk/` or AGENTS.md instructions.

## Current Project Context

Always inspect current state first:

```bash
ivk doctor --agent --json
```

## Golden Path

```bash
ivk ws new --from main --name <task>
ivk ws mount <workspace-id> ./workspaces/<workspace-id>
cd ./workspaces/<workspace-id>
# edit files
# run tests
ivk ch new <workspace-id>
ivk git export <changeset-id> --branch agent/<task>
```

## Critical Rules

- Do not edit the base repository directly.
- Do not create manual Git worktrees.
- Do not run `git checkout -b` for task isolation.
- Do not delete `.ivk/`.
- Do not push directly to Git unless instructed.
- Always use `ivk doctor --agent --json` when unsure.
```

---

## `skills/ivk/cli.md`

This file should be a precise command reference for agents.

Each command should include:

```text
purpose
arguments
flags
JSON output schema
exit codes
examples
common errors
recommended next command
```

---

## `skills/ivk/workflow.md`

This should describe complete workflows:

```text
single task workflow
failed attempt workflow
multi-agent parallel workflow
same-task multi-attempt workflow
changeset export workflow
cleanup workflow
```

---

## `skills/ivk/mcp.md`

MCP does not need to be implemented in MVP.

But the intended MCP interface should be documented early.

Future MCP tools:

```text
create_workspace
mount_workspace
list_workspaces
get_workspace_status
create_changeset
export_changeset_to_git
discard_workspace
run_gc
doctor
```

The MVP can ship `mcp.md` as a design/spec document.

Actual implementation can come later with:

```bash
ivk mcp serve
ivk mcp init
```

---

## `AGENTS.md` generator

Command:

```bash
ivk init --agent-instructions
```

Generates:

```text
AGENTS.md
```

Example content:

```md
# Agent Instructions for Ivy Kernel

This repository uses Ivy Kernel (`ivk`) for parallel AI-agent workspaces.

Do not create ad-hoc Git worktrees manually unless instructed.

For each task:

1. Create a workspace:
   `ivk ws new --from main --name <task-name>`

2. Materialize the workspace:
   `ivk ws mount <workspace-id> ./workspaces/<workspace-id>`

3. Make all code changes inside the mounted workspace.

4. Run tests inside the mounted workspace.

5. If the task succeeds, create a changeset:
   `ivk ch new <workspace-id>`

6. If the task fails, discard the workspace:
   `ivk ws rm <workspace-id>`

7. Do not directly push to Git. Export through:
   `ivk git export <changeset-id> --branch agent/<task-name>`
```

---

## JSON output everywhere

Every important command should support:

```bash
--json
```

Example:

```bash
ivk ws new --from main --name fix-login --json
```

Output:

```json
{
  "workspace_id": "ws_01HXYZ",
  "base_snapshot": "abc123",
  "status": "created",
  "mount_required": true,
  "next_command": "ivk ws mount ws_01HXYZ ./workspaces/ws_01HXYZ"
}
```

The `next_command` field is important because it helps agents continue correctly.

For more complex commands, use:

```json
{
  "recommended_next_steps": [
    "Run tests inside the mounted workspace",
    "Create a changeset with `ivk ch new ws_01HXYZ` if tests pass",
    "Discard the workspace with `ivk ws rm ws_01HXYZ` if the attempt failed"
  ]
}
```

---

## Agent-readable errors

Bad error:

```text
failed
```

Good error:

```json
{
  "error": "workspace_not_mounted",
  "message": "Workspace ws_123 is not mounted.",
  "recommended_next_command": "ivk ws mount ws_123 ./workspaces/ws_123"
}
```

---

## `ivk doctor --agent --json`

Example:

```json
{
  "repo_initialized": true,
  "inside_ivk_workspace": true,
  "workspace_id": "ws_123",
  "workspace_status": "active",
  "has_changes": true,
  "recommended_next_steps": [
    "Run project tests",
    "Create a changeset with `ivk ch new ws_123` if tests pass",
    "Discard with `ivk ws rm ws_123` if the attempt failed"
  ]
}
```

This is the `git status` equivalent for agents.

---

# 8. Measurement plan

## What to prove

The important proof is not simply:

```text
ivk is faster than Git.
```

The important proof is:

```text
AI agents can work in parallel with less workspace overhead, clearer lifecycle, easier cleanup, and better tool understanding.
```

## Core metrics

Workspace metrics:

```text
workspace_create_time_ms
workspace_materialize_time_ms
workspace_count
time_to_first_edit_ms
```

Storage metrics:

```text
total_disk_usage_bytes
workspace_disk_usage_bytes
overlay_size_bytes
duplicated_file_count
inode_count
cache_size_bytes
gc_reclaimed_bytes
```

Lifecycle metrics:

```text
workspace_discard_time_ms
gc_time_ms
stale_workspace_count
orphan_workspace_count
orphan_overlay_count
```

Change recovery metrics:

```text
changeset_create_time_ms
git_export_time_ms
successful_changesets
exported_branches
failed_workspaces
discarded_workspaces
```

Agent understanding metrics:

```text
agent_used_ivk_correctly
agent_modified_base_repo
agent_created_manual_git_worktree
agent_needed_human_intervention
agent_followed_next_command
agent_recovered_with_doctor
```

These can be measured in demos by observing whether the agent follows the intended workflow.

---

# 9. Benchmark design

## Benchmark 1: Workspace spawn

Counts:

```text
1, 5, 10, 25, 50, 100
```

Compare:

```text
git worktree
cp -r repo
ivk workspace
```

Measure:

```text
elapsed time
disk usage
inode count
cleanup time
```

Commands:

```bash
ivk bench spawn --count 100 --from main
ivk bench compare-git-worktree --count 100
```

---

## Benchmark 2: Dependency cache

Goal:

```text
Show that parallel workspaces do not need fully duplicated dependency worlds.
```

Targets:

```text
TypeScript + pnpm
Python + uv
Rust + Cargo
Flutter + Gradle
```

MVP target:

```text
TypeScript + pnpm
```

---

## Benchmark 3: 100-agent task throughput

Create 100 small tasks:

```text
src/tasks/task_001.ts
src/tasks/task_002.ts
...
src/tasks/task_100.ts
```

Each task has a test.

Measure:

```text
wall-clock time
successful changesets
failed workspaces
test pass rate
exported branches
cleanup time
disk usage before/after gc
```

---

## Benchmark 4: Failed agents are cheap

Demo:

```text
100 workspaces
30 successful
40 failed
30 canceled
```

Then:

```bash
ivk ws ls
ivk ws rm --failed
ivk gc
```

Message:

```text
AI agents create mess.
Ivy Kernel gives that mess a lifecycle.
```

---

## Benchmark 5: Agent understanding

Goal:

```text
Prove that an agent can learn ivk from AGENTS.md, SKILL.md, and CLI JSON output.
```

Procedure:

```text
1. Create fresh repo.
2. Run `ivk init --agent-instructions`.
3. Provide task to a coding agent.
4. Observe whether it uses ivk instead of raw Git.
5. Observe whether it stays inside the mounted workspace.
6. Observe whether it creates a changeset and exports through ivk.
```

Measure:

```text
agent_used_ivk_correctly: true/false
manual_git_worktree_created: true/false
base_repo_modified: true/false
changeset_created: true/false
git_export_created: true/false
human_intervention_count
```

This is a key v2 addition.

---

# 10. Demo plan

## Demo A: 100 agents, one repo

Flagship demo:

```text
100 AI agents work from the same base snapshot.
ivk creates isolated workspaces.
Successful changes become changesets.
Failed attempts are discarded.
Useful changes are exported to Git.
```

Commands:

```bash
ivk init --agent-instructions
ivk bench spawn --count 100 --from main
./scripts/run-agents.sh --parallel 100
ivk ch new --all
ivk ch ls
ivk git export --passed --branch-prefix agent/task-
ivk gc
```

---

## Demo B: Git worktree comparison

Run baseline:

```bash
./scripts/baseline-git-worktree.sh --count 100
```

Run ivk:

```bash
ivk bench compare-git-worktree --count 100
```

Show:

```text
workspace creation time
disk usage
cleanup time
stale workspace count
```

---

## Demo C: Agent learns ivk

This is the most important new demo in v2.

Flow:

```text
1. Fresh repo.
2. `ivk init --agent-instructions`.
3. AGENTS.md and skills/ivk files are present.
4. Ask a coding agent to complete a task.
5. Agent reads instructions.
6. Agent runs `ivk doctor --agent --json`.
7. Agent creates workspace.
8. Agent mounts workspace.
9. Agent edits only inside workspace.
10. Agent creates changeset.
11. Agent exports to Git.
```

Message:

```text
Ivy Kernel is not only a CLI.
It is an agent-readable workspace protocol.
```

---

## Demo D: Same task, many attempts

Run 10 agents on the same bug.

```text
workspace attempt 1
workspace attempt 2
...
workspace attempt 10
```

Pick the best passing changeset.

Message:

```text
Ivy Kernel makes alternative agent attempts cheap.
```

---

# 11. Landing page plan

Use GitHub Pages first.

Recommended domains:

```text
ivykernel.dev
ivy-kernel.dev
```

LP structure:

```text
Hero:
  Ivy Kernel
  Parallel workspaces for AI agents.
  Git makes branches cheap.
  Ivy Kernel makes workspaces cheap.

Problem:
  AI agents create many workspaces.
  Git worktrees make this possible, but not cheap or manageable enough.

Solution:
  base snapshot + agent workspaces + overlays + changesets

Agent-readable by design:
  AGENTS.md
  skills/ivk/SKILL.md
  ivk help --agent
  ivk doctor --agent --json
  JSON output with next_command
  future MCP

Benchmark:
  100 workspaces
  Git worktree vs ivk
  creation time
  disk usage
  cleanup time

Install:
  brew tap ivykernel/tap
  brew install ivk

Positioning:
  Git stores history.
  Jujutsu improves local change history.
  Ivy Kernel manages parallel AI workspaces.
```

Recommended site implementation:

```text
Astro
VitePress
or simple static HTML
```

Do not overbuild the LP.

---

# 12. Homebrew distribution plan

Use custom Homebrew tap first:

```text
ivykernel/homebrew-tap
```

Install command:

```bash
brew tap ivykernel/tap
brew install ivk
```

Use:

```text
cargo-dist
GitHub Releases
GitHub Actions
Homebrew tap update
```

Do not target Homebrew core initially.

Release flow:

```bash
cargo install cargo-dist
cargo dist init
git tag v0.1.0
git push origin v0.1.0
```

Then test:

```bash
brew tap ivykernel/tap
brew install ivk
ivk --version
ivk help --agent
```

---

# 13. GitHub Actions

CI:

```text
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo build
```

Release:

```text
cargo-dist generated release workflow
macOS Apple Silicon
macOS Intel
Linux x86_64
checksums
Homebrew tap update
```

Pages:

```text
GitHub Pages deployment for LP/docs
```

---

# 14. Development phases

## Pre-Phase: Benchmark spike (gating)

Before Phase 0, run the bench spike described in [`ivk_benchmark_spike.md`](./ivk_benchmark_spike.md).

Goal:

```text
Decide whether the "cheap workspaces" pitch is defensible by measuring
filesystem primitives (git worktree, cp -r, clonefile/reflink, hardlink,
overlayfs) against a realistic 100-workspace scenario, with and without
dependency sharing (pnpm).
```

Deliverable:

```text
results/summary.md
  - raw numbers per matrix cell
  - chosen materialization primitive (or: no winner; pivot pitch)
  - explicit go/no-go for Phases 0–9 below
```

Exit criteria:

```text
A documented decision exists for:
  (1) which working-tree materialization primitive wins on the target OS
  (2) whether dependency-store sharing (D3) is materially cheaper than D2
  (3) which pitch the product will lead with
```

If the spike says "no winner anywhere", do not proceed to Phase 0 until the pitch and Hypotheses (section 4) are revised.

---

## Phase 0: Project foundation and agent skill skeleton

Goal: create the repo and make `ivk` build as an agent-readable CLI.

Tasks:

- [x] create GitHub org/repo — `ivykernel` org reserved
- [x] create Rust workspace — `crates/{ivk-core,ivk-cli,clonewt}` + root Cargo.toml
- [ ] add clap CLI — currently hand-rolled; sufficient for now, port to clap when subcommand count grows
- [x] add serde JSON output — `serde` + `serde_json` in `ivk-cli`, envelope type in `output.rs`
- [ ] add tracing — deferred; `eprintln!` is sufficient for v0.0.1 and the doctor JSON already covers diagnostics
- [x] add CI — [`.github/workflows/ci.yml`](./.github/workflows/ci.yml) runs fmt + clippy + test + build on macOS and Linux
- [x] write README — [`README.md`](./README.md)
- [x] write design philosophy — [`ivk_design_philosophy.md`](./ivk_design_philosophy.md)
- [x] create AGENTS.md template — [`AGENTS.md`](./AGENTS.md)
- [x] create skills/ivk/SKILL.md — [`skills/ivk/SKILL.md`](./skills/ivk/SKILL.md)
- [x] create skills/ivk/cli.md — [`skills/ivk/cli.md`](./skills/ivk/cli.md)
- [x] create skills/ivk/workflow.md — [`skills/ivk/workflow.md`](./skills/ivk/workflow.md)
- [x] create skills/ivk/mcp.md — [`skills/ivk/mcp.md`](./skills/ivk/mcp.md)
- [x] create skills/ivk/rules/safety.md — [`skills/ivk/rules/safety.md`](./skills/ivk/rules/safety.md)
- [x] define JSON output convention — `Envelope { ok, command, next_command, recommended_next_steps?, error?, ...payload }`
- [x] define next_command convention — every JSON output sets `next_command` to the shell-form command an agent should run next (or `null` if terminal)

Commands:

- [x] `ivk --version`
- [x] `ivk help --agent`
- [x] `ivk doctor --agent --json`

Exit criteria:

- [x] `cargo test` passes — 3 integration tests in `crates/ivk-core/tests/integration.rs`
- [x] CI is configured (matrix runs on macOS + Linux). "CI passes" itself will only be true once the repo is pushed to GitHub.
- [x] README explains concept
- [x] agent skill files exist

---

## Phase 1: Init and agent policy

Goal: initialize `.ivk/` and generate agent instructions.

Commands:

- [x] `ivk init`
- [x] `ivk init --agent-instructions` (drops AGENTS.md + skills/ivk/* into the target repo via `include_str!`-embedded templates)
- [x] `ivk status` (+ `--json` + `--agent`)
- [x] `ivk doctor` (+ `--json` + `--agent`)
- [x] `ivk doctor --agent`

Files:

- [x] `.ivk/config.toml`
- [ ] `.ivk/db.sqlite` — deferred. For v0.0.1 the "registry" is just the directory layout under `.ivk/workspaces/`. SQLite arrives when we need cross-workspace transactional state.
- [x] `.ivk/agent-policy.toml`
- [x] `AGENTS.md` (template + generator)

Exit criteria:

- [x] fresh repo can be initialized
- [x] agent instructions are generated
- [x] agent doctor returns useful JSON

---

## Phase 2: Workspace lifecycle MVP

Commands:

- [x] `ivk ws new <name> [<name>...]` — creates one or more workspaces at `.ivk/workspaces/<name>`, positional names (no `--name`), `--from` defaults to HEAD
- [x] `ivk ws ls` (+ `--json` + `--agent`) — also reachable as `ivk ls`
- [x] `ivk ws show <name>` (+ `--json` + `--agent`) — also reachable as `ivk show`
- [ ] `ivk ws mount <name|id> [<path>]` — materialization is always to `.ivk/workspaces/<name>` in v0.0.1; explicit `mount` will come if we need workspaces outside `.ivk/`
- [x] `ivk ws diff <name>` (+ `--json`) — also reachable as `ivk diff`
- [x] `ivk ws rm <name> [<name>...]` (+ `--json`) — also reachable as `ivk rm`

Exit criteria:

- [x] can create N workspaces (verified for N up to 100 in spike)
- [x] can modify files inside each (verified end-to-end with the Vite fixture)
- [x] can list and remove them
- [x] JSON output works

---

## Phase 3: ChangeSet and Git export

Commands:

- [x] `ivk ch new <ws-name|ws-id>` — auto-commits inside the worktree, persists metadata to `.ivk/changesets/<id>.json`
- [x] `ivk ch ls`
- [x] `ivk ch show <ch-id>`
- [x] `ivk export <ch-id> [<branch>]` (short form; default branch `agent/<ws-name>`)
- [x] `ivk patch <ch-id> [<output>]` — default output `./patches/<ch-id>.patch`
- [ ] `ivk ship <ws-name|ws-id>` — convenience: ch new + export + git push + gh pr create. Deferred per [Risk 9](#risk-9-workspace--github-pr-is-a-four-step-dance); needs a separate spike on PR conventions and `gh` dependency before locking the workflow.

Exit criteria:

- [x] a modified workspace can become a changeset (verified end-to-end in `crates/ivk-cli/tests/changeset.rs`)
- [x] a changeset can become a Git branch
- [x] a patch can be generated

---

## Phase 4: Garbage collection

Commands:

- [x] `ivk gc [--dry-run]` — prune orphan workspace dirs + orphan `git worktree` admin entries; report `bytes_reclaimed`, `removed_workspaces`, `removed_admin`, `skipped_locked`, `orphaned_changeset_refs`. Holds `.ivk/.gc.lock` to serialize against bulk-rm. Never deletes `.ivk/changesets/*.json`.
- [x] `ivk ws rm --all [--yes] [--force] [--dry-run]` — bulk-remove every workspace; dirty workspaces are skipped without `--force`; requires `--yes` to confirm.
- [x] `ivk ws rm --exported [--yes] [--force] [--dry-run]` — remove workspaces whose HEAD matches a `refs/heads/agent/<ws>` ref in the source repo (work is already preserved on a branch).
- [ ] `ivk ws rm --failed` — deferred. v0.0.1 has no test-result tracking, so this would always have to fall back to manual filtering. Refuses today with `error.code = unsupported_flag` and points at `ivk ws rm --all --yes`.
- [ ] `ivk ws rm --all-discarded` — deferred. No exported/discarded marker exists in v0.0.1; `--exported` (preserved on a branch) + `--all` cover the realistic recovery flows. Refuses today with `error.code = unsupported_flag` and points at `ivk ws rm --exported --yes`.

Exit criteria:

- [x] N temporary workspaces can be removed cleanly (verified for N=30 in `crates/ivk-cli/tests/gc.rs::rm_all_removes_many_workspaces`; same code path runs for 100, and the disk-scaling spike already validated 100-workspace creation/deletion at the filesystem layer).
- [x] disk usage after gc is reported — `bytes_before` / `bytes_after` / `bytes_reclaimed` / `bytes_reclaimed_human` are always present in the `gc` JSON envelope (success and dry-run alike).

---

## Phase 5: Benchmarks

The core measurement work is already done as the pre-Phase spikes:
- [x] working-tree spike — [`results/summary.md`](./results/summary.md), 55–65× disk reduction, 5× faster create
- [x] build-artifact spike — [`results/build-summary.md`](./results/build-summary.md), 8–700× depending on toolchain
- [x] approach G (Rust prototype) validated as the MVP architecture

Remaining `ivk bench *` wrappers (replays of the spike scripts, exposed as subcommands):

- [x] `ivk bench spawn [--count N]` — pure-Rust; materializes N workspaces against cwd@HEAD into `.ivk/bench/<run>/`, reports total_wall_ms + p50/p90/p99 + apparent_kb + df-delta real_kb.
- [x] `ivk bench compare-git-worktree [--count N]` — randomized-order two-arm run; emits `comparison.lp_blurb` ready to paste into the LP.
- [x] `ivk bench disk [--count N]` — apparent / actual-blocks / df-delta triad + savings_ratio + lp_blurb.
- [x] `ivk bench gc [--count N]` — materializes N workspaces, breaks half, runs gc, reports `bytes_reclaimed` + ms-per-workspace. Distinct from `ivk gc`: uses a `gc-bench-<pid>-<ts>-ws-NNNN` prefix and never touches the user's pre-existing workspaces.
- [ ] `--from <rev>` (non-HEAD) — deferred. v0.0.1 only benches against HEAD because `ivk_core::materialize_workspace` is HEAD-only; adding non-HEAD support means exposing `clone_tree_only` from ivk-core which is a separate small refactor.
- [ ] `ivk bench matrix` (wraps `scripts/bench/collect.sh` for the full S/M/MD/L matrix) — deferred. The shell pipeline still runs against the dev checkout; the user-facing CLI ships pure-Rust to avoid imposing bash + perl + python3 on `cargo install ivk-cli` users. Will land once a Linux CI image proves the shell harness portable.

Exit criteria:

- [x] benchmark can compare ivk vs git worktree (proven)
- [x] results can be pasted into LP (chart and tables ready)

---

## Phase 6: Agent understanding demo

Goal: prove that a coding agent can learn ivk from repo-local instructions and CLI JSON output.

Deliver:

- [x] `AGENTS.md` — see Phase 0
- [x] `skills/ivk/SKILL.md` — see Phase 0
- [x] `skills/ivk/cli.md` — see Phase 0
- [x] `ivk doctor --agent --json` — see Phase 0
- [x] demo task — [`examples/demo-task/`](./examples/demo-task/) (off-by-one bug in a tiny Node.js fixture, with a failing test the agent must fix)
- [x] recorded agent session — [`examples/demo-task/session.md`](./examples/demo-task/session.md) captures the literal JSON output for every step of the golden path

Exit criteria:

- [x] agent uses ivk workflow without manual explanation — every step driven by `next_command` / `recommended_next_steps` from the previous JSON output (see `session.md`)
- [x] agent does not create raw Git worktree — only `ivk new` is used in the recorded session
- [x] agent does not edit base repo — `src/sum.js` is modified only inside `.ivk/workspaces/fix-sumto/`
- [x] agent creates changeset and exports through ivk — `ivk ch new` followed by `ivk export` produces `agent/fix-sumto` branch

---

## Phase 7: 100-agent demo repo

Deliver:

- [x] `examples/todo-100/` — fixture + scripts
- [x] 100 small TODO tasks — generated by `examples/todo-100/setup.sh` (`src/task_NNN.js` × 100, uniform off-by-one bug)
- [x] 100 tests — paired `test/task_NNN.test.js` × 100, all initially failing
- [x] scripts to simulate agents — [`examples/todo-100/simulate.sh`](./examples/todo-100/simulate.sh) with `--pass N --fail M` knobs and per-task CSV output
- [ ] scripts to run real coding agents later — deferred; the simulate.sh harness defines the exact ivk call sequence a real agent would issue, so a real-agent wrapper is mechanical once we pick an orchestration tool (Devin / repo-batch / homemade)

Exit criteria:

- [x] anyone can run the demo locally — `bash examples/todo-100/setup.sh && bash examples/todo-100/simulate.sh`
- [x] the demo creates workspaces, modifies files, creates changesets, exports branches, and runs gc — verified end-to-end: 100 workspaces materialized, 30 fixed + exported (30 `agent/task-NNN` branches), 20 broken + discarded, 50 abandoned + bulk-rm'd. Total wall time: ~37 s. Per-task CSV at `results/todo-100-<ts>/per-task.csv`.

---

## Phase 8: LP and docs

Single-page LP at [`docs/index.html`](./docs/index.html), styled via [`docs/styles.css`](./docs/styles.css), GitHub-Pages-ready via [`docs/_config.yml`](./docs/_config.yml). All sections fit on one scroll:

- [x] Home — hero + tagline
- [x] Install — brew tap + cargo build snippets
- [x] Quickstart — `ivk init --agent-instructions` + `ivk new attempt-{1,2,3}`
- [x] Agent Guide — section explaining `--agent --json` + `next_command` + linking to the recorded session
- [x] Skills — links to `AGENTS.md` and `skills/ivk/*.md`
- [x] Benchmarks — comparison table + embedded [`docs/disk-scaling.svg`](./docs/disk-scaling.svg)
- [x] Design — linked to [`ivk_design_philosophy.md`](./ivk_design_philosophy.md)
- [x] Roadmap — linked to this plan

Exit criteria:

- [x] GitHub Pages site is built (live status is true once the repo is pushed and Pages is enabled in repo settings: source = main branch, /docs)
- [x] README links to docs
- [ ] docs include demo GIF or terminal recording — [`demos/disk-comparison.tape`](./demos/disk-comparison.tape) is ready; rendering to GIF/MP4 requires running `vhs` locally (1 command, ~3 minutes). Left as a single manual step pre-launch so the recording has a stable filename for the LP to link.

---

## Phase 9: Release and Homebrew

Tasks:

- [x] configure cargo-dist — `[workspace.metadata.dist]` in [`Cargo.toml`](./Cargo.toml) declares targets (x86_64 + aarch64 × macOS + Linux), installers (shell + homebrew), and the tap (`ivykernel/homebrew-tap`).
- [ ] create GitHub release v0.1.0 — pending; procedure documented in [`docs/release.md`](./docs/release.md). Requires the repo to be pushed to GitHub first.
- [ ] create Homebrew tap (`ivykernel/homebrew-tap`) — pending; one-time bootstrap (`gh repo create ivykernel/homebrew-tap --public`) plus an empty `Formula/` directory; the first release PR will populate `Formula/ivk.rb` automatically.
- [ ] test brew install — pending; cannot run until the tap repo + a release exist.
- [x] document install path — [`docs/release.md`](./docs/release.md) walks through every step from `cargo dist init` to `brew install ivk`.

Install:

```bash
brew tap ivykernel/tap
brew install ivk
```

Exit criteria:

- [ ] a new user can install ivk on macOS using Homebrew and run `ivk init` — gated on the three pending tasks above, each of which requires the repo to be pushed to GitHub. From the local dev side everything cargo-dist needs is in place.

---

# 15. Risks and mitigations

## Risk 1: MVP still uses physical directories

Mitigation:

```text
Be transparent.
Phase 1 proves lifecycle, changeset flow, and agent readability.
Phase 2 optimizes materialization.
```

## Risk 2: Agents ignore ivk and use Git habits

Mitigation:

```text
AGENTS.md
skills/ivk/SKILL.md
ivk help --agent
ivk doctor --agent --json
JSON output with next_command
clear critical rules
```

## Risk 3: Git worktree baseline is already good

Mitigation:

```text
Measure lifecycle, cleanup, failed attempts, agent-readable protocol, and changeset recovery.
```

## Risk 4: Too much scope

Mitigation:

```text
Do not build IvyHub yet.
Do not build CI.
Do not build custom object store.
Do not build full MCP first.
```

## Risk 5: Homebrew distribution complexity

Mitigation:

```text
Use cargo-dist and a custom tap.
Do not target Homebrew core initially.
```

## Risk 6: Editor / IDE integration is missing

VS Code, Cursor, JetBrains all model "open one folder". A user running 10 parallel agent workspaces cannot reasonably open 10 editor windows. Without IDE integration, inspecting or editing inside a workspace requires manual `cd ~/.ivk/workspaces/ws-xyz/` and reopening the editor on that path — friction that erodes the DX win.

Mitigation:

```text
Phase 0–4: accept CLI-only inspection. Document the workflow in skills/ivk/workflow.md.
Phase 5+: ship a thin VS Code / Cursor extension that lists ivk workspaces and offers
          "open in new window" / "diff against base". Same for JetBrains plugin later.
Pitch:    until extensions exist, lead with the agent (CLI-native) use case, not the
          solo-human use case.
```

## Risk 7: Build artifacts duplicate per workspace

The bench validated the *working tree* layer. It did not validate the *build* layer. Each workspace runs its own `dist/`, `target/`, `.next/`, `build/`, etc. — these are fresh writes per workspace, not block-shared. For 10 workspaces each running a webpack/Vite/Cargo build, the build outputs can dwarf the working-tree savings.

pnpm and uv already solve this at the dependency level (content-addressed store). Build outputs do not have an equivalent default.

Mitigation:

```text
Phase 0–4: document toolchain-specific shared cache settings
           (CARGO_TARGET_DIR, TURBO_REMOTE_CACHE, Nx cache, etc.).
           ivk ws new can set these env vars if requested.
Phase 4+:  ivk-managed build cache layer — content-addressed shared cache
           that build tools can opt into. This is a real Phase 4+ engineering project.
Honesty:   the LP should not claim "100 workspaces in 1 GB" if a `pnpm build` in
           each workspace produces 100 × 200 MB of dist files. Lead with
           working-tree numbers and call out build artifacts as a separate axis.
```

## Risk 8: Tools assume one canonical working directory

File watchers, husky pre-commit hooks, language server caches, framework telemetry, project-rooted config files — many tools assume one canonical project path exists. Most will work transparently with N workspaces (each one is a valid project root), but some will misbehave in surprising ways, especially anything that writes to absolute paths or shares state across "the same project".

Mitigation:

```text
Phase 0:   dogfood ivk on a real Claude Code / Codex session early.
           Catalog every tool that breaks; document workarounds.
Phase 1+:  collect findings into skills/ivk/rules/safety.md so agents
           know which scenarios to avoid.
Acceptance: this risk is unbounded until dogfooded. Assume some breakage.
```

## Risk 9: Workspace → GitHub PR is a four-step dance

Current spec: `ivk ch new` → `ivk git export --branch foo` → `git push` → `gh pr create`. Four commands to get an agent's work into review. Compare to today's `git push -u origin HEAD && gh pr create`. ivk loses on ergonomics here.

Mitigation:

```text
Phase 3:   add `ivk ship <workspace>` convenience command that runs
           ch new + git export + git push + gh pr create in one.
           Accept flags to customize each step.
Default:   the long-form commands stay available for scripting and edge cases.
           ivk ship is the human/agent-friendly shortcut.
```

---

# 16. Immediate next actions

Order is bench-first. Steps 1–4 produce a defensible pitch *before* any Rust is written.

## Step 1: Create repo and bench harness

```text
ivykernel/ivk          GitHub org ivykernel is already reserved
  scripts/bench/       shell scripts only — no Rust yet
  results/
  README.md            short: "this is a workspace kernel; spike in progress"
```

## Step 2: Run the benchmark spike

Per [`ivk_benchmark_spike.md`](./ivk_benchmark_spike.md):

```text
scripts/bench/gen-repo.sh      generate S/M/L synthetic repos
scripts/bench/bench-create.sh  approaches A/B/C × sizes × N
scripts/bench/bench-deps.sh    D1/D2/D3 with pnpm
scripts/bench/collect.sh       full matrix, emit CSV
```

Time budget: 3–5 days.

## Step 3: Write `results/summary.md`

Decide:

```text
which materialization primitive wins (or none does)
whether dependency-store sharing is the real wedge
which pitch the product leads with
```

## Step 4: Reconcile the plan with results

If the spike validates "cheap": continue to Step 5 unchanged.
If it pivots the pitch: update sections 4 (Hypotheses), 9 (Benchmark design),
and 11 (Landing page) in this doc before continuing.

## Step 5: Initialize Rust project

```bash
cargo new ivk --bin
```

Add dependencies:

```text
clap
serde
serde_json
toml
anyhow
thiserror
tracing
tracing-subscriber
tempfile
rusqlite              SQLite from day 1 — concurrent agent writes break JSON
```

## Step 6: Add agent skill skeleton

Create:

```text
skills/ivk/SKILL.md
skills/ivk/cli.md
skills/ivk/workflow.md
skills/ivk/mcp.md
skills/ivk/rules/safety.md
```

## Step 7: Implement CLI skeleton

```bash
ivk --version
ivk help --agent
ivk init --agent-instructions
ivk status --json
ivk doctor --agent --json
```

## Step 8: Implement workspace registry

```text
.ivk/db.sqlite         not registry.json
.ivk/config.toml
.ivk/agent-policy.toml
```

## Step 9: Implement workspace lifecycle

```bash
ivk ws new
ivk ws ls
ivk ws mount           uses the materialization primitive chosen in Step 3
ivk ws diff
ivk ws rm
```

## Step 10: Implement changeset / export

```bash
ivk ch new
ivk ch show
ivk git export
```

## Step 11: Wire bench scripts into `ivk bench`

```bash
ivk bench spawn
ivk bench compare-git-worktree
```

These wrap the Step 2 shell scripts so the LP and CI can re-run them.

## Step 12: Build demo repo

```text
examples/todo-100
```

## Step 13: Publish LP

```text
GitHub Pages
ivykernel.dev or ivy-kernel.dev
```

## Step 14: Release

```text
cargo-dist
GitHub Release
Homebrew tap (ivykernel/homebrew-tap)
```

---

# 17. Final product narrative

`ivk` should not be positioned as a Git replacement at first.

It should be positioned as:

```text
a local-first, agent-readable workspace kernel for AI-agent development
```

The clean story is:

```text
Git stores history.
Jujutsu makes local change history easier.
Ivy Kernel manages parallel AI workspaces.
Ivy Kernel also teaches agents how to use those workspaces safely.
IvyHub will review and collaborate on Ivy changesets later.
```

The first public proof should be:

```text
100 agents.
1 repo.
100 isolated workspaces.
Agent-readable instructions.
Successful changesets exported to Git.
Failed work discarded.
Disk reclaimed.
```

That is enough to justify the project.
