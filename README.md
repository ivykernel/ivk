# Ivy Kernel (`ivk`)

**A parallel workspace kernel for AI agents.**

```text
Git makes branches cheap.
Git worktree makes working trees possible.
Ivy Kernel makes 100 parallel working trees fit in the disk of one
— and creates them faster than git worktree does.
```

## One-line concept

`ivk` lets many AI agents work on the same codebase in parallel, each in its own lightweight, lifecycle-managed workspace, with the working tree block-shared via APFS `clonefile(2)` on macOS or `cp --reflink` on Linux (btrfs / xfs / zfs).

## Status

**v0.0.2 shipped** (Homebrew tap live, full workspace/changeset/gc lifecycle, 44 tests green). The MVP plan ([v2](./ivk_mvp_to_launch_plan_v2.md)) is complete.

Current work: evolving `ivk` from a CLI tool into a **workspace kernel usable from multiple frontends** (desktop CLI today; an iOS frontend via libgit2 + FFI next). See [`ivk_workspace_kernel_plan_v3.md`](./ivk_workspace_kernel_plan_v3.md) for the reorganized roadmap.

## Why

Git was designed for a single human working on one task at a time. AI-agent workflows are different: many agents work in parallel, most attempts are throwaway, and each needs an isolated workspace. Plain `git worktree` works but is expensive — each worktree is a full checkout of every file.

`ivk` solves this by:

1. Sharing the source repo's `.git/` directory via the standard git worktree pointer mechanism (just like `git worktree add` does).
2. Materializing the working tree files via filesystem-level copy-on-write (`clonefile(2)` / `FICLONE`), which is free at create time and stays free until you write to a block.
3. Tracking workspaces as lifecycle-managed kernel objects, not just directories.

The result, measured on a realistic 600 MB TypeScript project at 100 parallel workspaces:

| | git worktree | **ivk** | savings |
|---|---:|---:|---:|
| disk | 64.85 GB | **1.00 GB** | **65× less** |
| create time (serial) | 4.58 min | **50 s** | **5.4× faster** |

See [`results/summary.md`](./results/summary.md) and [`results/build-summary.md`](./results/build-summary.md) for the full benchmark write-ups.

## Quickstart

```bash
# Install (Homebrew tap, Phase 9):
brew tap ivykernel/tap
brew install ivk

# Or build from source:
git clone https://github.com/ivykernel/ivk
cd ivk
cargo build --release --workspace
./target/release/ivk --version
```

In any git repository:

```bash
# Create one or more workspaces from HEAD:
ivk new attempt-1 attempt-2 attempt-3
ivk new attempt-{1,2,3}            # equivalent (bash brace expansion)

# Move into one:
cd .ivk/workspaces/attempt-1
# ... edit, build, test as normal — it IS a real git worktree

# Check state at any time:
ivk doctor --agent --json
```

## Repo layout

```text
crates/
  ivk-core/       library: materialize_workspace + filesystem primitive
  ivk-cli/        the `ivk` binary
  clonewt/        bench harness (thin wrapper around ivk-core)
scripts/bench/    reproducible benchmarks (working-tree + build-artifact spikes)
demos/            LP material (vhs recording script + SVG chart generator)
docs/             portability and design notes
skills/ivk/       agent-readable docs (SKILL.md, cli.md, workflow.md, ...)
results/          benchmark numbers, decision documents
```

## Filesystem support

| OS | FS | Status |
|---|---|---|
| macOS | APFS | ✅ default; uses `clonefile(2)` |
| Linux | btrfs / xfs(reflink) / zfs ≥ 2.2 | ✅ via `cp --reflink=always` |
| Linux | ext4 | ❌ no reflink; overlayfs fallback planned |
| Windows | — | out of scope (MVP) |

Details in [`docs/portability.md`](./docs/portability.md).

## Agent use

`ivk` is designed to be used by AI agents (Claude Code, Codex, Cursor, etc.) as much as by humans. Every important command supports `--json` for structured output and `--agent` for an agent-friendly summary with a `next_command` field that tells the agent what to run next.

If your repo contains [`AGENTS.md`](./AGENTS.md) plus [`skills/ivk/`](./skills/ivk/), an agent can learn the workflow without prior knowledge of `ivk`. See those files for the full contract.

## Benchmarks

The decision to build `ivk` is backed by two reproducible spikes:

- [`ivk_benchmark_spike.md`](./ivk_benchmark_spike.md) — working-tree materialization. Validated: 55–65× disk reduction, 5× faster create vs `git worktree`.
- [`ivk_build_artifact_spike.md`](./ivk_build_artifact_spike.md) — what happens when each workspace actually builds. Validated: TS 8–37× cheaper, Rust 700× cheaper with clonefile preserving cargo's incremental cache.

Reproduce locally:

```bash
bash scripts/bench/gen-repo.sh M
bash scripts/bench/bench-create.sh A M 100      # baseline
bash scripts/bench/bench-create.sh G M 100      # ivk
python3 scripts/bench/analyze.py results/raw/*/bench-create.csv
```

## License

Dual-licensed under MIT or Apache-2.0.

## Roadmap pointer

The active roadmap (multi-frontend workspace kernel: backend traits, SQLite state, libgit2, iOS FFI) lives in [`ivk_workspace_kernel_plan_v3.md`](./ivk_workspace_kernel_plan_v3.md). The completed MVP plan is preserved in [`ivk_mvp_to_launch_plan_v2.md`](./ivk_mvp_to_launch_plan_v2.md).
