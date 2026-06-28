# Ivy Kernel — Benchmark Spike Plan

> **STATUS: DONE (2026-06-25).** Headline result: clonefile + git-worktree
> hybrid wins on disk by **55–65×** and on create time by **5×** vs
> `git worktree`. Full numbers + tables: [`results/summary.md`](./results/summary.md).
> The decision rule in §"Pass/fail criteria" returned the "cheap pitch
> holds" outcome and Phase 0 was built around the validated primitive.
> This doc is retained as the spike's design record.

## Purpose

Before building any of the `ivk` CLI, kernel, registry, or agent-readability layer, validate the core technical premise:

> *Parallel workspaces can be made materially cheaper than `git worktree add` × N — in time, disk, or both — on realistic repositories.*

If this holds, build the kernel around the winning primitive.
If it does not, pivot the pitch from **"cheap workspaces"** to **"lifecycle + agent-readability"** and adjust the MVP plan accordingly.

This spike is intentionally **CLI-less**. It uses shell scripts (and optionally a tiny Rust helper) to measure the *underlying filesystem primitives*. No `.ivk/` registry, no `ivk init`, no skill files. The goal is to learn the answer in days, not weeks.

---

## Why this gate exists

The current MVP plan (`ivk_mvp_to_launch_plan_v2.md`) materializes workspaces by copying / checking out files into managed directories and deriving overlays from `git diff`. In that form, **`ivk` is functionally a metadata wrapper around `git worktree`** until Phase 2+ optimizations (CoW / overlay / shared cache) land.

If we ship Phase 1 and run the headline demo (100 agents, 100 workspaces), the benchmark numbers may show `ivk` matching or losing to plain `git worktree`. That would directly contradict the public pitch ("Git makes branches cheap. Ivy Kernel makes workspaces cheap.") and burn the launch.

The fix is to find the winning primitive **before** building around the wrong one.

---

## Hypothesis

> For N parallel workspaces materialized from one base commit, at least one materialization strategy is **≥ 2× cheaper** than `git worktree add` × N on either time-to-N-workspaces or disk-after-N-workspaces, without regressing the other by more than 20%, on a realistic-sized repository (10k+ files) at N=100.

If no strategy clears this bar, the "cheap workspaces" claim is not defensible and the product must be repositioned.

---

## Approaches to compare

```text
A. git worktree add                       (baseline)
B. cp -r                                  (naive deep copy)
C. cp -c   on macOS / cp --reflink=auto   (APFS clonefile / btrfs/xfs reflink)
D. cp -al                                 (hardlink tree)
E. overlayfs / FUSE                       (Linux only; skip in v1 of spike)
```

MVP will pick the winner from C/D/E for the target platform. macOS first (APFS `clonefile` is the strongest candidate), then Linux.

---

## Repo sizes

Synthesize three repos with realistic file-size distribution and commit history.

```text
S:   1k files,  ~10 MB    (toy)
M:  10k files, ~200 MB    (typical web app)
L: 100k files,   ~2 GB    (monorepo)
```

Generator script: `scripts/bench/gen-repo.sh`. Uses a deterministic seed so runs are reproducible.

---

## N workspaces

```text
1, 10, 50, 100
```

---

## Metrics

### Primary (gating)

```text
create_total_ms       wall clock to create N workspaces
disk_after_create_b   apparent + actual disk usage after N workspaces exist
cleanup_total_ms      wall clock to remove all N workspaces
inodes_used           df -i delta
```

### Secondary (collected, not gating)

```text
time_to_first_edit_ms   touch one file in workspace 0, measure CoW fault cost
peak_memory_b           RSS of the materialization process
warm_cache_create_ms    second run, FS cache warm
```

---

## Dependency cache scenario

The "cheap workspaces" claim collapses if every workspace ships its own 1 GB `node_modules`. This must be measured explicitly — it is likely the *real* cost driver for AI-agent workflows.

```text
D1: no dependencies                 plain repo, baseline
D2: pnpm install per workspace      no sharing
D3: pnpm install once, shared store symlinked into each workspace
```

The interesting comparison is **D2 vs D3**. If D3 is dramatically cheaper on disk while keeping tools (`pnpm run test`) functional in each workspace, *that* may be the real product, not the working-tree CoW.

Target stack for the spike: TypeScript + pnpm. Other ecosystems (uv, Cargo, Gradle) deferred until after the spike validates direction.

---

## Pass/fail criteria

The spike must answer three questions:

1. **Working tree materialization**: Does any of C/D/E clear the 2× bar over A at N=100, size=M, scenario D1?
2. **Dependency sharing**: Does D3 clear the 2× bar over D2 on disk at N=100, size=M?
3. **Combined**: best-of(C/D/E) + D3 vs A + D2 — what is the realistic ivk-vs-baseline gap on a 100-workspace scenario?

### Decision rule

```text
(1) PASS  AND  (2) PASS    →  Ship the "cheap workspaces" pitch.
                              Build Phase 1 around the winning primitive.

(1) FAIL  AND  (2) PASS    →  Pitch becomes "cheap dependency sharing
                              + lifecycle + agent-readability".
                              Working tree stays a thin git-worktree wrapper.

(1) FAIL  AND  (2) FAIL    →  Repitch to "lifecycle + agent-readability".
                              Workspaces are not the wedge; agent UX is.
                              De-scope Benchmark phase from the MVP plan.
```

---

## Deliverables

```text
ivykernel/ivk/
  scripts/bench/
    gen-repo.sh              synthesize S/M/L repos
    bench-create.sh          measure approach × size × N (create + first edit)
    bench-deps.sh            measure D1/D2/D3
    bench-cleanup.sh         measure cleanup time
    collect.sh               run full matrix, emit CSV
  results/
    raw/<timestamp>/*.csv    per-run raw data
    summary.md               human-readable writeup with decision
```

The single artifact that matters at the end of the spike is `results/summary.md` — one document stating:

```text
Here are the numbers.
Here is the pitch we can defend.
Here is the primitive we will build on.
```

---

## Out of scope for the spike

```text
ivk CLI / clap / subcommands
.ivk/ registry (JSON or SQLite)
AGENTS.md / skills/ivk
ChangeSet / Git export
Garbage collection
MCP
Cross-platform parity (macOS first; Linux added only if macOS results justify it)
Windows entirely
```

If any of these would be required to run the bench fairly, write the smallest possible shim. Do not build the full feature.

---

## Timeline

3–5 working days. Not weeks.

```text
Day 1   gen-repo.sh + bench-create.sh for A/B/C, sizes S/M
Day 2   scenarios D1/D2/D3 with pnpm
Day 3   size L, N=100, complete full matrix
Day 4   write results/summary.md, make pitch decision
Day 5   buffer for re-runs / anomalies / Linux pass if macOS passed
```

If day 4 ends without a clear answer, the spike has uncovered a more interesting question than expected. Stop and re-scope before continuing.

---

## What happens after

### If the spike validates "cheap"

Resume `ivk_mvp_to_launch_plan_v2.md` from Phase 0 (project foundation), with the winning primitive baked in from day 1. Section 5 (Benchmark) in the MVP plan becomes a *re-run at scale for LP screenshots*, not a research question.

### If the spike pivots the pitch

1. Update `ivk_design_philosophy.md` short pitch and "Why Ivy Kernel exists" sections.
2. Update `ivk_mvp_to_launch_plan_v2.md` section 4 (Hypotheses 1–3) and section 11 (Landing page hero).
3. De-emphasize Phase 5 (Benchmarks) — the bench is already done.
4. Push Phase 6 (Agent understanding demo) and the lifecycle story to the front of the narrative.

In either case, the spike output (`results/summary.md`) is checked into the repo as the historical record of why the product is shaped the way it is.

---

## Operational notes

- Run on a dedicated machine when possible. Don't bench on a laptop running Slack and Chrome.
- Record OS / kernel / filesystem / hardware in every CSV header. macOS APFS vs Linux ext4 vs btrfs will diverge wildly.
- Drop FS caches between runs (`sync && purge` on macOS, `echo 3 > /proc/sys/vm/drop_caches` on Linux) to measure cold-start honestly.
- Always also measure warm-cache. Real agent workflows hit warm cache.
- Each cell of the matrix: median of 3 runs. Discard the slowest if it's > 2σ from the other two.
- Keep the spike scripts in `scripts/bench/` even after the MVP starts — they become the regression suite.
