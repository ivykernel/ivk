# Ivy Kernel — Build Artifact Sharing Spike Plan

## Purpose

The [working-tree spike](./ivk_benchmark_spike.md) proved that **filesystem-level CoW** (APFS clonefile / reflink) makes N parallel workspaces cheap on disk for the *source files*. It explicitly did **not** measure what happens once each workspace runs a build.

```text
ivk_benchmark_spike.md     answered:  cheap working trees?         → YES (55×)
ivk_build_artifact_spike   answers:   cheap build artifacts too?   → TBD
```

This is the next spike. It must run **before** the LP claims "100 workspaces in 1 GB" without qualification — otherwise the headline is true only for repos that never get built.

Risk reference: [Risk 7](./ivk_mvp_to_launch_plan_v2.md#risk-7-build-artifacts-duplicate-per-workspace) in the MVP plan.

---

## The problem

For a real TS / Rust / Go / Java project, the dominant disk cost is rarely the source tree. It is the build output:

```text
project/
  src/                ~10–200 MB    (cheap via clonefile)
  node_modules/       ~200 MB–2 GB  (cheap if pnpm shared store, expensive otherwise)
  dist/ / .next/      ~50–500 MB    (fresh write per workspace, NOT shared)
  target/             ~100 MB–3 GB  (Cargo, fresh per workspace by default)
  build/ / .gradle/   ~500 MB–5 GB  (Gradle/Android, fresh per workspace)
  __pycache__/        small         (cheap)
  .pytest_cache/      small         (cheap)
```

If 10 parallel workspaces each run `pnpm build`, the 10× `dist/` cost may dwarf the working-tree savings.

The clonefile-based approach **does not** automatically share build outputs, because:
- Build tools write fresh files; they don't `cp -c` from somewhere.
- Each workspace's build is incremental against its own state, not a shared baseline.
- Sharing requires either (a) toolchain-level shared cache config, or (b) post-build re-deduplication.

---

## Hypothesis

> For a representative TS/JS web project and a representative Rust project, the build artifact cost across N parallel workspaces can be reduced by **≥ 5×** using a combination of: shared dependency stores (pnpm, uv, Cargo registry), shared build caches (CARGO_TARGET_DIR, TURBO_REMOTE_CACHE), and `ivk`-managed cache mounts.

If true, the LP headline survives intact at "100 realistic project workspaces in a few GB".
If false, the LP must qualify: "for repos using shared dependency caches, on the working tree only".

---

## Approaches to compare

For each toolchain, measure baseline vs sharing strategies:

### TypeScript / pnpm (representative web project)

```text
T1. baseline:   pnpm install + pnpm build, no sharing
T2. shared store: pnpm install with single global ~/.pnpm-store (default)
                  + per-workspace node_modules symlink tree (default)
                  + per-workspace dist/    (no sharing on builds)
T3. shared dist: T2 + content-addressed dist/ via clonefile after build
T4. turbo remote-cache: T2 + Turborepo with local file cache as remote
```

Target project shape: ~500 deps, ~200 MB source, ~150 MB built `.next/`.

### Rust / Cargo

```text
R1. baseline:   cargo build per workspace, default target/
R2. shared target: CARGO_TARGET_DIR=$HOME/.cache/ivk/cargo-target
                   shared across all workspaces
R3. sccache:    R1 + sccache as compiler cache
R4. shared+sccache: R2 + R3
```

Target project shape: ~50 deps, ~10 MB source, ~800 MB target/ on first build.

### Optional: Go, Gradle, Python

Include if time permits. The TS and Rust cases cover the dominant agent workflows.

---

## Metrics

```text
Per-workspace metrics (replicated across N):
  build_time_cold_ms        first-ever build, no caches warm
  build_time_warm_ms        second build, caches warm
  build_artifact_size_kb    size of dist/ or target/ for one workspace
  disk_real_kb              df delta after creating N workspaces and building each

Aggregate metrics:
  total_disk_after_N_builds_kb
  build_concurrency_safe    boolean: do parallel builds break (lock contention, etc.)
```

---

## Pass/fail criteria

```text
Headline (TS): 10 parallel workspaces each running `pnpm build`
  baseline (T1):           must reach X GB
  best sharing strategy:   must reach < X/5 GB

Headline (Rust): 10 parallel workspaces each running `cargo build`
  baseline (R1):           must reach Y GB
  best sharing strategy:   must reach < Y/5 GB

Concurrency:
  At least one sharing strategy per toolchain must support parallel builds without
  data races or lock contention that breaks build correctness.
```

If both 5× targets pass: ship the integrated story. ivk recommends specific config per toolchain.
If only TS passes: ship with TS as the canonical demo, document Rust as "Phase 2".
If neither passes: build artifact sharing becomes its own engineering project, not an ivk feature.

---

## Out of scope for this spike

```text
Distributed build caches (BuildBuddy, Bazel remote cache)
Container-layer build caches (BuildKit)
Cross-machine sharing
Network-mounted shared volumes
Custom Cargo/pnpm forks
```

The spike must answer "does shared local cache work for one developer running N agents?" Not "does Bazel scale to a 1000-engineer org?".

---

## Deliverables

```text
scripts/bench/build/
  setup-ts.sh           generate a representative TS project (Next.js or Vite)
  setup-rust.sh         generate a representative Cargo workspace
  bench-build-ts.sh     measure T1–T4
  bench-build-rust.sh   measure R1–R4
  collect-build.sh      run full matrix
results/build/
  raw/<ts>/
  summary.md            human-readable decision
```

The single artifact that matters at the end: `results/build/summary.md` saying:

```text
For TS:   use strategy Tx, achieves Z× reduction.
For Rust: use strategy Rx, achieves W× reduction.
LP claim adjustment: ___
ivk feature implications: ___ (e.g. inject CARGO_TARGET_DIR env var on ws mount)
```

---

## Timeline

5–7 working days.

```text
Day 1   set up representative TS project (Next.js with realistic deps)
Day 2   bench T1–T4 on TS, cold + warm
Day 3   set up Rust project, bench R1–R4
Day 4   parallel-build concurrency check; gather metrics
Day 5   write results/build/summary.md, decide
Day 6–7 buffer / re-runs
```

This spike is gated on the Rust ivk-hybrid (approach G) benchmark completing first. The order:

```text
working-tree spike (done)
  ↓
Rust F prototype + bench (in progress)
  ↓
build-artifact spike  ← this doc
  ↓
ivk Phase 0 with informed pitch
```

---

## What we change in ivk based on results

Possible outcomes:

1. **`ivk ws new --tool <name>` flag** that knows about common toolchains and sets the right shared-cache env vars (CARGO_TARGET_DIR, TURBO_*, PNPM_STORE_DIR, UV_CACHE_DIR, etc.).
2. **`ivk cache` subcommand** for managing the ivk-owned shared cache directory.
3. **A documented "best practices" page** with copy-paste config snippets per toolchain, if no in-product feature is justified.
4. **A revised LP claim** if 5× cannot be hit reliably across toolchains.

The spike output decides which.
