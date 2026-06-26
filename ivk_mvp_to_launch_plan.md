# Ivy Kernel (`ivk`) MVP-to-Launch Plan

## Purpose

This document defines a practical development plan for **Ivy Kernel (`ivk`)**, a parallel workspace kernel for AI agents.

The goal is to build, measure, demonstrate, document, and distribute `ivk` until it is credible as a developer tool that can be installed with Homebrew and understood by coding agents.

---

## Core positioning

### One-line positioning

**Ivy Kernel (`ivk`) is a parallel workspace kernel for AI agents.**

### Why it exists

Git makes branching cheap.

But in AI-agent development, branching is not enough.

AI agents need many isolated working spaces:

```text
Agent A -> workspace A
Agent B -> workspace B
Agent C -> workspace C
...
Agent N -> workspace N
```

With plain Git, this usually becomes many worktrees or many cloned directories.

That creates problems:

```text
heavy workspace creation
duplicated dependencies
duplicated build caches
stale directories
manual cleanup
hard-to-track failed attempts
unclear agent state
```

Ivy Kernel exists to make this manageable.

### Key message

```text
Branches are cheap.
Worktrees are not.
Ivy Kernel makes workspaces cheap too.
```

### Comparison

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

### Strategic statement

```text
Jujutsu makes one developer's local history easier.
Ivy Kernel makes many agents' parallel workspaces cheap and manageable.
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

## Rust stack

Recommended initial stack:

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

Benchmarking:
  criterion for microbenchmarks
  custom `ivk bench` for product benchmarks

Testing:
  assert_cmd
  predicates
  tempfile
  insta for snapshot testing if useful

Release:
  cargo-dist
  GitHub Actions
  Homebrew tap
```

## Why shell out to Git first?

Do not overbuild Git integration in MVP.

For the first version, `ivk` can call the local `git` binary.

This reduces complexity and keeps compatibility obvious.

Examples:

```text
git rev-parse HEAD
git diff
git worktree add for baseline comparison
git checkout
git commit-tree or git commit
git branch
git format-patch
```

Later, if performance or portability becomes an issue, `ivk` can move selected operations to a Rust Git library such as `gix`.

---

# 2. Product architecture

## Minimal kernel responsibilities

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

## Separation between `ivk` and IvyHub

The boundary should be:

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

## Recommended GitHub structure

Recommended organization:

```text
ivykernel
```

Recommended repositories:

```text
ivykernel/ivk        # Rust CLI and kernel
ivykernel/docs       # Documentation / GitHub Pages site, optional initially
ivykernel/homebrew-tap # Homebrew tap, when release starts
ivykernel/ivyhub     # Future hosted review layer
```

If starting under the company organization first:

```text
TemmaTech/ivk
```

But the long-term public OSS identity should be:

```text
ivykernel/ivk
```

## Initial repo structure

```text
ivk/
  Cargo.toml
  README.md
  LICENSE
  CHANGELOG.md
  docs/
    design.md
    agent-protocol.md
    benchmarks.md
    demo.md
  crates/
    ivk-cli/
    ivk-core/
    ivk-git/
    ivk-agent/
    ivk-bench/
  examples/
    todo-100/
  scripts/
    demo-100-agents.sh
    baseline-git-worktree.sh
  .github/
    workflows/
      ci.yml
      release.yml
```

## Crate structure

For MVP, avoid too many crates if it slows development.

Recommended practical structure:

```text
crates/ivk-cli
crates/ivk-core
```

Then split later.

### `ivk-cli`

Handles:

```text
command parsing
human-readable output
JSON output
exit codes
```

### `ivk-core`

Handles:

```text
workspace registry
snapshot model
overlay model
changeset model
filesystem operations
Git shell commands
```

Later optional crates:

```text
ivk-git
ivk-agent
ivk-bench
ivk-cache
```

---

# 4. MVP feature set

## MVP must prove three hypotheses

### Hypothesis 1

`ivk` can create many isolated workspaces faster and cheaper than plain Git worktree workflows.

### Hypothesis 2

`ivk` can track, discard, and garbage-collect failed agent workspaces cleanly.

### Hypothesis 3

`ivk` can convert successful workspace results into Git-compatible changesets.

---

## MVP commands

### Project initialization

```bash
ivk init
ivk init --agent-instructions
```

Creates:

```text
.ivk/
  config.toml
  registry.json or registry.sqlite
  workspaces/
  overlays/
  changesets/
AGENTS.md
```

### Workspace commands

```bash
ivk ws new --from main --name fix-login
ivk ws ls
ivk ws show <workspace-id>
ivk ws mount <workspace-id> ./workspaces/<workspace-id>
ivk ws diff <workspace-id>
ivk ws rm <workspace-id>
```

### ChangeSet commands

```bash
ivk ch new <workspace-id>
ivk ch ls
ivk ch show <changeset-id>
```

### Git export commands

```bash
ivk git export <changeset-id> --branch agent/fix-login
ivk git patch <changeset-id> --output ./patches/fix-login.patch
```

### Status and diagnosis

```bash
ivk status
ivk doctor
ivk doctor --agent
```

### Benchmark commands

```bash
ivk bench spawn --count 100 --from main
ivk bench compare-git-worktree --count 100
ivk bench disk
ivk bench gc
```

### Cleanup

```bash
ivk gc
```

---

# 5. Internal data model

## Snapshot

In MVP:

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

MVP implementation can be simple.

At first, a mounted workspace may be a physical directory.

The overlay can be derived from Git diff against the base snapshot.

Later, replace or optimize with actual overlay/copy-on-write behavior.

### MVP overlay strategy

Use this pragmatic approach:

```text
1. Create lightweight workspace metadata.
2. Materialize by copying or checking out files into a managed directory.
3. Compute overlay by diffing against base snapshot.
4. Store overlay metadata and file patches.
```

This does not yet fully solve physical duplication.

But it proves lifecycle and changeset workflow first.

### Phase 2 overlay strategy

Improve with:

```text
hardlinks
reflinks on supported filesystems
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

Start with a local file:

```text
.ivk/registry.json
```

Move to SQLite if needed.

MVP recommendation:

```text
Use JSON first for speed of development.
Use SQLite when concurrent writes become real.
```

Because parallel agents may write concurrently, add file locking early if using JSON.

---

# 6. Agent understanding layer

Git and GitHub are already well-known to coding agents.

`ivk` is not.

Therefore, `ivk` must be agent-readable from day one.

The goal is:

```text
An agent should be able to discover the correct ivk workflow from repo files and CLI output.
```

## Required agent understanding features

### 1. `AGENTS.md` generator

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

### 2. `ivk help --agent`

Agent-focused help should be concise, imperative, and workflow-oriented.

Example:

```text
You are an AI coding agent using Ivy Kernel.

Use isolated ivk workspaces instead of modifying the base repository directly.

Typical workflow:
  ivk ws new --from <branch-or-snapshot> --name <task>
  ivk ws mount <workspace-id> <path>
  cd <path>
  make changes
  run tests
  ivk ch new <workspace-id>
  ivk git export <changeset-id> --branch agent/<task>

Rules:
  - Never edit files outside the mounted workspace.
  - Never create Git worktrees manually.
  - Always create a changeset before exporting.
  - Discard failed workspaces with `ivk ws rm`.
```

### 3. JSON output everywhere

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

The `next_command` field is important.

It helps agents continue correctly.

### 4. `ivk doctor --agent --json`

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

### 5. Agent policy file

Generate:

```text
.ivk/agent-policy.toml
```

Example:

```toml
[agent]
default_base = "main"
workspace_root = "./workspaces"
branch_prefix = "agent/"
require_tests_before_changeset = false
allow_git_direct_push = false
allow_manual_worktree = false

[export]
default_remote = "origin"
default_target = "git_branch"

[cleanup]
auto_gc_after_export = true
```

### 6. Agent-readable errors

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

## Agent demo

Demo title:

```text
An agent learns ivk from the repository
```

Demo flow:

```text
1. Run `ivk init --agent-instructions`.
2. Open repo with Codex / Claude Code / Cursor.
3. Ask the agent to fix a task.
4. Agent reads AGENTS.md.
5. Agent uses `ivk ws new`, `ivk ws mount`, `ivk ch new`.
6. Agent exports a Git branch.
```

This demonstrates that `ivk` is not only a CLI.

It is an agent-readable workspace protocol.

---

# 7. Measurement plan

## What to prove

The important proof is not simply:

```text
ivk is faster than Git.
```

The important proof is:

```text
AI agents can work in parallel with less workspace overhead, clearer lifecycle, and easier cleanup.
```

## Core metrics

### Workspace creation metrics

```text
workspace_create_time_ms
workspace_materialize_time_ms
workspace_count
time_to_first_edit_ms
```

### Storage metrics

```text
total_disk_usage_bytes
workspace_disk_usage_bytes
overlay_size_bytes
duplicated_file_count
inode_count
cache_size_bytes
gc_reclaimed_bytes
```

### Lifecycle metrics

```text
workspace_discard_time_ms
gc_time_ms
stale_workspace_count
orphan_workspace_count
orphan_overlay_count
```

### Change recovery metrics

```text
changeset_create_time_ms
git_export_time_ms
successful_changesets
exported_branches
failed_workspaces
discarded_workspaces
```

### Parallelism metrics

```text
parallel_workspace_count
agent_start_latency_ms
agent_completion_time_ms
throughput_changesets_per_hour
max_parallel_agents
```

### Quality metrics

```text
test_pass_rate
conflict_count
average_diff_size
successful_export_rate
```

---

# 8. Benchmark design

## Benchmark 1: Workspace spawn

Goal:

```text
Show that ivk can create many managed workspaces quickly.
```

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

Command:

```bash
ivk bench spawn --count 100 --from main
ivk bench compare-git-worktree --count 100
```

Expected output format:

```text
Benchmark: 100 parallel workspaces

Baseline: git worktree
  create time: 482s
  disk usage: 8.7GB
  cleanup time: 94s

Ivy Kernel:
  create time: 38s
  disk usage: 1.2GB
  cleanup time: 4s

Result:
  12.6x faster creation
  7.2x less disk
  23.5x faster cleanup
```

Numbers above are placeholders. Replace with actual measurements.

---

## Benchmark 2: Dependency cache

Goal:

```text
Show that parallel workspaces do not need fully duplicated dependency worlds.
```

Good demo targets:

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

Measure:

```text
install time
test start latency
node_modules duplication
shared cache usage
total disk usage
```

---

## Benchmark 3: 100-agent task throughput

Goal:

```text
Show that ivk can manage many independent agent attempts.
```

Create 100 small tasks.

Example:

```text
src/tasks/task_001.ts
src/tasks/task_002.ts
...
src/tasks/task_100.ts
```

Each file has a TODO.

Each task has a test.

Run:

```text
100 workspaces
100 agent attempts
collect changesets
export passed changes
discard failed attempts
gc
```

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

Goal:

```text
Show that failed parallel work can be discarded cheaply.
```

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

Measure:

```text
cleanup time
remaining disk
reclaimed disk
recoverable changesets
orphan workspace count
```

Message:

```text
AI agents create mess.
Ivy Kernel gives that mess a lifecycle.
```

---

# 9. Demo plan

## Demo A: 100 agents, one repo

This is the flagship demo.

### Setup

```bash
git clone <demo-repo>
cd <demo-repo>
ivk init --agent-instructions
```

### Create 100 workspaces

```bash
ivk bench spawn --count 100 --from main
```

### Run agents

In MVP, the agent runner can be external.

```bash
./scripts/run-agents.sh --parallel 100
```

`ivk` does not need to orchestrate agents yet.

### Collect changes

```bash
ivk ch new --all
ivk ch ls
```

### Export passed changes

```bash
ivk git export --passed --branch-prefix agent/task-
```

### Cleanup

```bash
ivk gc
```

### Demo message

```text
100 AI agents worked from the same base snapshot.
Ivy Kernel created isolated workspaces, collected successful changesets,
exported them to Git, and garbage-collected failed work.
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

Message:

```text
Git makes branches cheap.
Ivy Kernel makes workspaces cheap.
```

---

## Demo C: Agent learns ivk

### Flow

```text
1. Create a fresh repo.
2. Run `ivk init --agent-instructions`.
3. Ask a coding agent to fix a task.
4. Agent reads AGENTS.md.
5. Agent uses ivk commands.
6. Agent produces a changeset and Git branch.
```

This proves the Agent Understanding Layer.

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

# 10. Landing page plan

## Recommended hosting

Use GitHub Pages first.

Why:

```text
simple
free
developer-native
works well with open source repos
custom domains supported
easy to deploy from GitHub Actions
```

## Domain options

Recommended:

```text
ivykernel.dev
ivy-kernel.dev
ivk.dev if available, but likely not ideal
```

If using GitHub Pages before custom domain:

```text
https://ivykernel.github.io/ivk/
```

## LP structure

### Hero

```text
Ivy Kernel

Parallel workspaces for AI agents.

Git makes branches cheap.
Ivy Kernel makes workspaces cheap.
```

CTA:

```text
Install
View GitHub
Watch Demo
Read Design
```

### Section 1: Problem

```text
AI agents create many workspaces.
Git worktrees make this possible, but not cheap or manageable enough.
```

Visual:

```text
100 agents -> 100 messy worktrees
```

### Section 2: Solution

```text
Ivy Kernel manages agent workspaces as lifecycle-managed objects.
```

Visual:

```text
base snapshot
  + overlay A
  + overlay B
  + overlay C
```

### Section 3: How it works

```bash
ivk init --agent-instructions
ivk ws new --from main --name fix-login
ivk ws mount ws_123 ./workspaces/ws_123
ivk ch new ws_123
ivk git export ch_123 --branch agent/fix-login
ivk gc
```

### Section 4: Agent-readable by design

```text
AGENTS.md generator
ivk help --agent
ivk doctor --agent
JSON output
next_command hints
```

### Section 5: Benchmark

Show measured results.

```text
100 workspaces
Git worktree vs ivk
creation time
disk usage
cleanup time
```

### Section 6: Positioning

```text
Git stores history.
Jujutsu improves local change history.
Ivy Kernel manages parallel AI workspaces.
```

### Section 7: Install

```bash
brew tap ivykernel/tap
brew install ivk
```

Or after formula is accepted / stable:

```bash
brew install ivk
```

### Section 8: Roadmap

```text
MVP
cache-aware workspaces
overlay materialization
IvyHub
MCP server
```

## Tech choice for LP

Start simple.

Recommended:

```text
Astro + GitHub Pages
```

or even:

```text
VitePress
```

If you want fastest:

```text
single-page static HTML + Tailwind
```

Do not overbuild the LP.

---

# 11. Homebrew distribution plan

## Recommended approach

Use a custom Homebrew tap first.

Recommended tap:

```text
ivykernel/homebrew-tap
```

Install command:

```bash
brew tap ivykernel/tap
brew install ivk
```

Homebrew taps are the standard way to distribute third-party formulae before inclusion in Homebrew core.

## Release automation

Recommended tool:

```text
cargo-dist
```

Why:

```text
builds release artifacts
generates installers
supports GitHub Releases
can generate Homebrew installer/tap workflows
works well for Rust CLI apps
```

## Release pipeline

### Step 1: Configure Cargo metadata

In `Cargo.toml`:

```toml
[package]
name = "ivk"
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
repository = "https://github.com/ivykernel/ivk"
description = "A parallel workspace kernel for AI agents"
```

### Step 2: Add cargo-dist

```bash
cargo install cargo-dist
cargo dist init
```

Configure:

```text
GitHub Releases
macOS Apple Silicon
macOS Intel
Linux x86_64
Homebrew installer
```

### Step 3: Create Homebrew tap

```text
github.com/ivykernel/homebrew-tap
```

### Step 4: Release with GitHub tag

```bash
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions builds binaries and updates release assets.

### Step 5: Install test

```bash
brew tap ivykernel/tap
brew install ivk
ivk --version
```

## Before Homebrew core

Do not try to enter Homebrew core immediately.

Use tap first.

Core inclusion requires stronger maturity, stable releases, user demand, and formula standards.

---

# 12. GitHub Actions

## CI workflow

Run on every PR:

```text
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo build
```

Also run:

```text
integration tests
CLI snapshot tests
basic benchmark smoke tests
```

## Release workflow

Use cargo-dist generated workflow.

Artifacts:

```text
ivk-aarch64-apple-darwin.tar.xz
ivk-x86_64-apple-darwin.tar.xz
ivk-x86_64-unknown-linux-gnu.tar.xz
ivk-aarch64-unknown-linux-gnu.tar.xz
checksums
Homebrew formula update
```

## Pages workflow

Deploy LP/docs to GitHub Pages.

Options:

```text
docs/ directory
gh-pages branch
GitHub Actions static site deployment
```

---

# 13. Development phases

## Phase 0: Project foundation

Goal:

```text
Create the repository and make ivk build.
```

Tasks:

```text
create GitHub org/repo
create Rust workspace
add clap CLI
add serde JSON output
add tracing
add CI
write README
write design philosophy
write AGENTS.md template
```

Commands implemented:

```bash
ivk --version
ivk help
ivk help --agent
```

Exit criteria:

```text
cargo test passes
CI passes
README explains concept
```

---

## Phase 1: Local registry and init

Goal:

```text
Initialize .ivk and generate agent instructions.
```

Commands:

```bash
ivk init
ivk init --agent-instructions
ivk status
ivk doctor
ivk doctor --agent
```

Files:

```text
.ivk/config.toml
.ivk/registry.json
.ivk/agent-policy.toml
AGENTS.md
```

Exit criteria:

```text
Fresh repo can be initialized.
Agent instructions are generated.
Agent doctor returns useful JSON.
```

---

## Phase 2: Workspace lifecycle MVP

Goal:

```text
Create, list, show, mount, diff, remove workspaces.
```

Commands:

```bash
ivk ws new --from main --name task-001
ivk ws ls
ivk ws show <id>
ivk ws mount <id> ./workspaces/<id>
ivk ws diff <id>
ivk ws rm <id>
```

Implementation:

```text
Use Git commit hash as base snapshot.
Use managed workspace directories.
Compute diffs against base snapshot.
Track status in registry.
```

Exit criteria:

```text
Can create 10 workspaces.
Can mount them.
Can modify files inside each.
Can list and remove them.
JSON output works.
```

---

## Phase 3: ChangeSet and Git export

Goal:

```text
Convert workspace result into changeset and export to Git.
```

Commands:

```bash
ivk ch new <workspace-id>
ivk ch ls
ivk ch show <changeset-id>
ivk git export <changeset-id> --branch agent/task-001
ivk git patch <changeset-id> --output task-001.patch
```

Exit criteria:

```text
A modified workspace can become a changeset.
A changeset can become a Git branch.
A patch can be generated.
```

---

## Phase 4: Garbage collection

Goal:

```text
Make failed agent work cheap to clean up.
```

Commands:

```bash
ivk gc
ivk ws rm --failed
ivk ws rm --all-discarded
```

Metrics:

```text
reclaimed bytes
removed workspaces
remaining active workspaces
orphan overlays
```

Exit criteria:

```text
100 temporary workspaces can be removed cleanly.
Disk usage after gc is reported.
```

---

## Phase 5: Benchmarks

Goal:

```text
Prove the performance story.
```

Commands:

```bash
ivk bench spawn --count 100 --from main
ivk bench compare-git-worktree --count 100
ivk bench disk
ivk bench gc
```

Outputs:

```text
human-readable table
JSON output
CSV output optional
```

Exit criteria:

```text
Benchmark can compare ivk vs git worktree.
Results can be pasted into LP.
```

---

## Phase 6: Demo repo

Goal:

```text
Create a reproducible 100-agent demo.
```

Repo:

```text
examples/todo-100
```

Contents:

```text
100 small TODO tasks
100 tests
scripts to simulate agents
scripts to run real coding agents later
```

Demo command:

```bash
./scripts/demo-100-agents.sh
```

Exit criteria:

```text
Anyone can run the demo locally.
The demo creates workspaces, modifies files, creates changesets, exports branches, and runs gc.
```

---

## Phase 7: LP and docs

Goal:

```text
Publish public-facing explanation and benchmark results.
```

Pages:

```text
Home
Install
Quickstart
Agent Guide
Benchmarks
Design
Roadmap
```

Exit criteria:

```text
GitHub Pages site is live.
README links to docs.
Docs include demo GIF or terminal recording.
```

---

## Phase 8: Release and Homebrew

Goal:

```text
Install ivk with Homebrew.
```

Tasks:

```text
configure cargo-dist
create GitHub release v0.1.0
create Homebrew tap
test brew install
document install path
```

Install:

```bash
brew tap ivykernel/tap
brew install ivk
```

Exit criteria:

```text
A new user can install ivk on macOS using Homebrew and run `ivk init`.
```

---

# 14. Suggested roadmap timeline

This is a practical sequence, not a hard schedule.

## Milestone 1: Prototype CLI

Deliver:

```text
Rust CLI skeleton
ivk init
AGENTS.md generation
ivk help --agent
ivk doctor --agent --json
```

Proof:

```text
Agent-readable protocol exists.
```

## Milestone 2: Workspace MVP

Deliver:

```text
workspace create/list/mount/diff/remove
registry
JSON output
```

Proof:

```text
10 workspaces can exist and be managed.
```

## Milestone 3: ChangeSet export

Deliver:

```text
changeset creation
git branch export
patch export
```

Proof:

```text
A successful agent workspace can be converted back to Git.
```

## Milestone 4: 100 workspace benchmark

Deliver:

```text
ivk bench spawn
git worktree baseline
disk/cleanup metrics
```

Proof:

```text
ivk has measurable workspace-management advantage.
```

## Milestone 5: Agent demo

Deliver:

```text
100 TODO demo
Agent instructions
external agent runner script
changeset collection
gc
```

Proof:

```text
AI-agent parallel development workflow is understandable and demonstrable.
```

## Milestone 6: Public preview

Deliver:

```text
GitHub Pages LP
README
recorded demo
Homebrew tap
GitHub release
```

Proof:

```text
Developers can install, understand, and try ivk.
```

---

# 15. README first draft outline

```md
# Ivy Kernel (`ivk`)

Parallel workspaces for AI agents.

Git makes branches cheap. Ivy Kernel makes workspaces cheap.

## Install

brew tap ivykernel/tap
brew install ivk

## Quickstart

ivk init --agent-instructions
ivk ws new --from main --name fix-login
ivk ws mount ws_123 ./workspaces/ws_123
cd ./workspaces/ws_123
# make changes
ivk ch new ws_123
ivk git export ch_123 --branch agent/fix-login

## Why

AI agents create many parallel attempts.
Git worktrees make this possible, but they are not lifecycle-managed enough for large-scale agent work.

## Agent-readable by design

ivk generates AGENTS.md and provides JSON output, `ivk help --agent`, and `ivk doctor --agent`.

## Benchmarks

100 workspace benchmark coming soon.
```

---

# 16. Risks and mitigations

## Risk 1: MVP still uses physical directories

This weakens the initial "lightweight overlay" story.

Mitigation:

```text
Be transparent.
Phase 1 proves lifecycle and changeset flow.
Phase 2 optimizes materialization with hardlinks/reflinks/overlay.
```

## Risk 2: Git worktree baseline is already good

Mitigation:

```text
Focus not only on raw speed.
Measure lifecycle, cleanup, agent-readable protocol, changeset recovery, failed attempt discard.
```

## Risk 3: Agent does not follow instructions

Mitigation:

```text
AGENTS.md
ivk help --agent
ivk doctor --agent
JSON output with next_command
clear forbidden actions
```

## Risk 4: Too much scope

Mitigation:

```text
Do not build IvyHub yet.
Do not build CI.
Do not build custom object store.
Do not build MCP first.
```

## Risk 5: Homebrew distribution complexity

Mitigation:

```text
Use cargo-dist and a custom tap.
Do not target Homebrew core initially.
```

---

# 17. Immediate next actions

## Step 1: Create repo

```text
ivykernel/ivk
```

or temporarily:

```text
TemmaTech/ivk
```

## Step 2: Initialize Rust project

```bash
cargo new ivk --bin
```

Add:

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
```

## Step 3: Implement CLI skeleton

```bash
ivk --version
ivk help --agent
ivk init --agent-instructions
ivk status --json
ivk doctor --agent --json
```

## Step 4: Implement workspace registry

```text
.ivk/registry.json
.ivk/config.toml
.ivk/agent-policy.toml
```

## Step 5: Implement workspace lifecycle

```bash
ivk ws new
ivk ws ls
ivk ws mount
ivk ws diff
ivk ws rm
```

## Step 6: Implement changeset/export

```bash
ivk ch new
ivk ch show
ivk git export
```

## Step 7: Implement benchmarks

```bash
ivk bench spawn
ivk bench compare-git-worktree
```

## Step 8: Build demo repo

```text
examples/todo-100
```

## Step 9: Publish LP

```text
GitHub Pages
ivykernel.dev or ivy-kernel.dev
```

## Step 10: Release

```text
cargo-dist
GitHub Release
Homebrew tap
```

---

# 18. Final product narrative

`ivk` should not be positioned as a Git replacement at first.

It should be positioned as:

```text
a local-first workspace kernel for AI-agent development
```

The clean story is:

```text
Git stores history.
Jujutsu makes local change history easier.
Ivy Kernel manages parallel AI workspaces.
IvyHub will review and collaborate on Ivy changesets later.
```

The first public proof should be:

```text
100 agents.
1 repo.
100 isolated workspaces.
Successful changesets exported to Git.
Failed work discarded.
Disk reclaimed.
Agent workflow readable from AGENTS.md.
```

That is enough to justify the project.
