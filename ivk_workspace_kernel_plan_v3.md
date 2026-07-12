# Ivy Kernel (`ivk`) Workspace-Kernel Plan v3 — multi-frontend

## Purpose

v2 ([`ivk_mvp_to_launch_plan_v2.md`](./ivk_mvp_to_launch_plan_v2.md)) took `ivk`
from zero to a shipped CLI (`v0.0.2` on Homebrew, LP live, 44 tests green).
v2 remains the record of that MVP.

v3 answers the next strategic question, raised by an architecture review of the
project:

> `ivk` should evolve from a "CLI tool" into a **workspace kernel usable from
> multiple frontends** — the desktop CLI today, an iOS product (CodeOn, a
> renamed Blink fork) next, and a hosted review layer (IvyHub) later. iOS
> cannot spawn an external `git` binary, so the kernel needs a `GitBackend`
> abstraction with a libgit2 implementation — *alongside*, not replacing, the
> Git CLI backend on desktop.

**We agree with the review's direction.** This document is the reorganized
roadmap: what we adopt as-is, what we adjust, what we defer, and in what order
we build it.

---

## Decision record

### Adopted as proposed

| Proposal | Verdict | Rationale |
|---|---|---|
| Split into `ivk-core` / `GitBackend` / `MaterializerBackend` / `ivk-cli` / `ivk-ffi` | ✅ adopt | Every downstream item (libgit2, FFI, SQLite transactions) needs this seam. Desktop behavior is unchanged; risk is low. |
| Add libgit2 **without** deleting the Git CLI backend | ✅ adopt | Desktop: the real `git` binary stays the compatibility baseline. iOS: libgit2 is the only option (no process spawning). Tests run the same operations against both backends and diff the results. |
| libgit2 op priority: clone/fetch → credential callback → branch/ref → checkout → status → diff → worktree → patch; commit explicit-only; push off by default | ✅ adopt | Matches the CodeOn MVP cut ("plain GitHub repo, HTTPS auth, branches, diff, keep uncommitted changes"). |
| Defer LFS, full submodules, partial clone, sparse checkout, complex credential helpers, rebase/interactive rebase, merge conflict editor | ✅ adopt | Each is a multi-week rabbit hole; none is needed for the CodeOn MVP or the desktop agent workflow. |
| SQLite registry, crash/transaction recovery, creation journal, workspace locks, storage estimation | ✅ adopt | Mobile force-kill makes this mandatory, and the desktop story wants it too: today's registry is "the directory layout", which cannot represent in-flight state. v2 explicitly deferred SQLite "until we need cross-workspace transactional state" — that moment is now. |
| Snapshot incl. untracked files, patch+untracked export/import, stale-vs-base indication, per-workspace agent-thread metadata, no-commit/no-push policy, Issue/PR association | ✅ adopt | All are kernel-level facts (not UI), consistent with the "ivk creates facts" boundary. |
| ivk = workspace engine, CodeOn = mobile product, IvyHub = future review/sharing service | ✅ adopt | Same separation v2 already drew for IvyHub, extended to a second frontend. |
| Rust core built for `aarch64-apple-ios` / `aarch64-apple-ios-sim` / `x86_64-apple-ios`, packaged as an XCFramework, called from Swift | ✅ adopt | Standard, proven toolchain path (UniFFI). |

### Adopted with adjustments

| Proposal | Adjustment | Why |
|---|---|---|
| `GitBackend` trait shape (`clone_repository`, `resolve_revision`, `create_branch`, `create_workspace`, `status`, `diff`, `create_patch`, `remove_workspace`) | Same surface, but **two altitudes**: git-level primitives live on the trait (`resolve_revision`, `status`, `diff`, `create_patch`, `add_worktree`, …); `create_workspace` / `remove_workspace` are **kernel compositions** in `ivk-core` that orchestrate `GitBackend` + `Materializer`. `clone_repository`/fetch/credentials join the trait in Phase C together with the credential design — shipping a clone API without a credential story is half an API. | A workspace is git worktree admin *plus* CoW materialization *plus* lifecycle state; putting it on the git trait would force libgit2 to know about clonefile. |
| libgit2 via the `git2` crate | Adopt `git2` (rust-lang maintained; worktrees, credential callbacks, diff/patch all covered). Keep `gix` (pure Rust) on a watch list — if iOS cross-compilation of libgit2+TLS becomes painful, gix removes the C dependency entirely, but its worktree support is not yet at parity. | Choose the boring option, document the exit. |
| APFS clone on iOS: "let Swift `FileManager.copyItem` do it" | Keep **both doors open** behind the `Materializer` trait: a Darwin `clonefile(2)`-based Rust materializer (shared with macOS) *and* the option for CodeOn to inject a Swift-side copy callback via FFI. Decide during CodeOn MVP integration, based on sandbox behavior on-device. Apple documents that high-level copy APIs on APFS clone automatically, so either path preserves the CoW win. | The trait makes the choice cheap; hardcoding it now is premature. |
| "Codex thread ID per workspace" | Generalize to an **agent-session metadata map** on the workspace record (`agent`, `session_id`/`thread_id`, `issue_url`, `pr_url`, free-form KV). Codex thread ID is one key, not a schema column. | ivk serves Claude Code / Codex / Cursor / others; the kernel should not hardcode one vendor. |
| "commit only as explicit operation, push disabled by default" | Already true operationally (`ivk ch new` is the only committing command and is always explicit; ivk never pushes — it only prints `git push …` as a suggested next step). Phase E turns this from convention into **enforced policy**: `agent-policy.toml` gains `allow_commit` / `allow_push` knobs that backends enforce, so an FFI frontend cannot bypass them. | The policy file exists since v0.0.1 but nothing enforces it yet. |

### Explicitly out of scope for ivk (this repo)

- **CodeOn itself** — the Blink fork, its rename, SwiftUI/Runestone UI, App
  Store submission. Tracked in the CodeOn repo. ivk's deliverable to CodeOn is
  `ivk-ffi` (XCFramework) plus this contract: see [Phase D](#phase-d-ivk-ffi--xcframework-v04x).
  Two facts the review confirmed, recorded here so they are not re-litigated:
  - The Blink fork **must not ship under the Blink name** (trademark); CodeOn
    is the working name.
  - Blink is GPLv3. App Store distribution of GPLv3 code is only safe with
    the copyright holders' blessing or a store-exception clause; CodeOn must
    resolve this (or reduce Blink usage) before submission. GPL compliance +
    Apple review are CodeOn-side launch gates, not ivk gates. `ivk` itself is
    MIT/Apache-2.0 dual-licensed, and `ivk-ffi` linking libgit2 (GPLv2 *with
    linking exception*) is compatible with that.
- **IvyHub** — unchanged from v2: hosted review/collaboration comes after the
  kernel is multi-frontend.

---

## Target architecture

```text
ivk (this repo)
├─ crates/ivk-core          the kernel: no CLI, no FFI, no UI
│   ├─ workspace model      create / status / remove compositions
│   ├─ changeset model      record / export / patch
│   ├─ lifecycle + GC       journal, crash recovery, locks
│   ├─ registry             SQLite (Phase B); today: directory layout
│   ├─ agent protocol       next_command / recommended_next_steps types
│   ├─ git backends         trait GitBackend
│   │    ├─ GitCliBackend   shells out to git; desktop default & baseline
│   │    └─ Libgit2Backend  feature "libgit2"; iOS / embedded    (Phase C)
│   └─ materializers        trait Materializer
│        ├─ CowMaterializer     macOS clonefile(2) / Linux FICLONE
│        ├─ CopyMaterializer    plain recursive copy fallback
│        └─ (host-injected)     optional FFI callback for Swift copyfile (Phase D)
│
├─ crates/ivk-cli           the `ivk` binary (thin: parsing + JSON envelopes)
├─ crates/ivk-ffi           UniFFI bindings → XCFramework for CodeOn  (Phase D)
└─ crates/clonewt           bench harness (unchanged)

CodeOn  (separate repo)     iOS product; Swift/SwiftUI; links ivk-ffi
IvyHub  (future)            hosted review layer over changesets
```

The `GitBackend` trait, target shape (Phase A ships the subset the CLI uses
today; Phase C adds the network half):

```rust
pub trait GitBackend {
    fn name(&self) -> &'static str;

    // Phase A — local operations (in use by ivk-cli today)
    fn resolve_revision(&self, repo: &Path, rev: &str) -> Result<String, GitError>;
    fn resolve_revision_short(&self, repo: &Path, rev: &str) -> Result<String, GitError>;
    fn status(&self, worktree: &Path) -> Result<StatusSummary, GitError>;
    fn diff_stat(&self, repo: &Path, target: DiffTarget) -> Result<DiffStat, GitError>;
    fn diff_patch(&self, repo: &Path, target: DiffTarget, binary: bool) -> Result<Vec<u8>, GitError>;
    fn add_worktree(&self, repo: &Path, dst: &Path, rev: &str) -> Result<(), GitError>;
    fn populate_index(&self, worktree: &Path, rev: &str) -> Result<(), GitError>;
    fn remove_worktree(&self, repo: &Path, worktree: &Path) -> Result<(), GitError>;
    fn prune_worktrees(&self, repo: &Path) -> Result<Vec<String>, GitError>;
    fn stage_all_and_commit(&self, worktree: &Path, message: &str, identity: &CommitIdentity)
        -> Result<String, GitError>;                       // the ONLY committing op
    fn create_branch(&self, repo: &Path, branch: &str, sha: &str, force: bool) -> Result<(), GitError>;
    fn list_refs(&self, repo: &Path, prefix: &str) -> Result<Vec<RefEntry>, GitError>;

    // Phase C — network + checkout (needed by CodeOn, implemented for both backends)
    // fn clone_repository(&self, url, dst, opts: CloneOptions) -> Result<(), GitError>;
    // fn fetch(&self, repo, remote, opts: FetchOptions) -> Result<(), GitError>;
    // fn checkout(&self, worktree, rev) -> Result<(), GitError>;
    //   CloneOptions/FetchOptions carry a CredentialProvider callback (HTTPS + token first).
    // push: NOT on the trait until a policy layer exists to gate it (Phase E).
}
```

---

## Priority stack — what ships in what order, and why

```text
P0  Phase A  Backend seam        traits + GitCliBackend + Materializer; CLI rewired; zero behavior change
P1  Phase B  Durable core state  SQLite registry, creation journal, crash recovery, locks, storage estimation
P2  Phase C  Libgit2Backend      feature-gated; op list above; dual-backend parity test suite
P3  Phase D  ivk-ffi             UniFFI + XCFramework; iOS targets building in CI
P4  Phase E  Kernel features     untracked-aware snapshot/export, stale detection, policy enforcement, metadata
P5  Phase F  Frontend enablement CodeOn integration support; IvyHub prep
```

Ordering rationale:

1. **A before everything** — it is the seam every other line item plugs into,
   and it is the only phase that can be done with provably zero user-visible
   change (the existing test suite is the referee).
2. **B before C (durable state before libgit2)** — the review's own "needed
   early, especially mobile" list is mostly Phase B. A parity test suite (C)
   and an FFI surface (D) are only meaningful against a registry with real
   transactional semantics; and desktop gains immediately (today two
   concurrent `ivk` processes race on directory scans, and a SIGKILL during
   `ivk new` leaves a half-materialized directory that only `gc` can explain).
3. **C before D** — the FFI should export a kernel that already proves both
   backends agree, otherwise every CodeOn bug becomes "kernel or backend?".
4. **E after C/D** — untracked-file snapshots and policy enforcement must be
   implemented against *both* backends at once, or the parity suite forks.

---

## Phase A: backend seam (v0.1.0) — DONE

Goal: `ivk-core` owns every git and filesystem operation behind traits;
`ivk-cli` becomes a thin frontend. No behavior change.

- [x] `GitBackend` trait + `GitError` / `StatusSummary` / `DiffStat` /
      `DiffTarget` / `RefEntry` / `CommitIdentity` types in `ivk-core::git`
- [x] `GitCliBackend` — consolidates the ~20 `Command::new("git")` call sites
      previously scattered across `ws.rs` / `ch.rs` / `gc.rs` / `doctor.rs` /
      `status.rs` / `lib.rs`
- [x] `Materializer` trait; existing clonefile/FICLONE code becomes
      `CowMaterializer`; new `CopyMaterializer` (plain recursive copy) as the
      explicit fallback for non-CoW filesystems — *not* auto-selected;
      selection policy is Phase B config
- [x] `materialize_workspace_with(git, materializer, opts)`;
      `materialize_workspace(opts)` kept as a compatibility wrapper
- [x] workspace removal composition (`worktree remove --force` → `rm -rf`
      fallback → prune) moves to `ivk-core` (deduplicates `ws.rs` / `gc.rs`)
- [x] `ivk-cli` rewired; bench code intentionally keeps shelling out to `git`
      (it *benchmarks* the real git binary; that is the point)
- [x] strategy naming unified: doctor/status report the materializer's
      strategy string (fixes stale `linux-reflink-via-cp` label vs the actual
      FICLONE implementation)
- [x] backend + materializer integration tests in ivk-core
      (`crates/ivk-core/tests/backend.rs` — becomes the Phase C parity suite)

Exit criteria:

- [x] zero `Command::new("git")` outside `GitCliBackend`, bench, and test fixtures
- [x] all pre-existing tests pass unchanged (44 → 48 with the new backend
      tests); fmt + clippy clean
- [x] `ivk-core` builds with no new required dependencies (libgit2 not yet pulled in)

Bonus fix surfaced by the new tests: `git worktree prune --verbose` reports
removals on **stderr**, so the v0.0.x gc parser (stdout-only) left
`pruned_admin_entries` always empty. `GitCliBackend::prune_worktrees` scans both
streams; the field now populates as documented.

## Phase B: durable core state (v0.2.x)

Goal: kernel state survives concurrent agents and force-kills. This is the
"especially for mobile, add early" list from the review, built desktop-first.

- [ ] `.ivk/db.sqlite` via `rusqlite` (bundled) — workspace + changeset
      registry; WAL mode; the directory layout stays the physical source of
      files, SQLite becomes the source of *state*
- [ ] migration: on first run, backfill DB from directory scan +
      `changesets/*.json` (JSON files remain as export artifacts)
- [ ] creation journal: `ivk new` records `creating → ready`; anything found
      mid-`creating` at startup is finished or rolled back (`ivk doctor`
      reports it; `ivk gc` reclaims it)
- [ ] transactional recovery after SIGKILL: every multi-step op (new, ch new,
      rm, gc) journals intent first; `ivk doctor --repair` completes or
      reverts
- [ ] per-workspace lock (supersedes today's coarse `.gc.lock`; git's
      worktree `locked` marker remains respected)
- [ ] storage estimation: `ivk ws du` / doctor fields — apparent size vs
      CoW-shared blocks per workspace (reuses bench `disk.rs` machinery)
- [ ] `ivk ws rm --failed` / `--all-discarded` un-deferred: the DB now has the
      state (`status`, `exported_at`) that v0.0.x lacked
- [ ] `--from <rev>` for `ivk new` (the Phase A trait already takes `rev`;
      wire it through the CLI)
- [ ] `tracing` behind `-v` / `IVK_LOG` (journal debugging wants it)

Exit criteria: kill -9 during `ivk new`/`ch new` at any point → next `ivk
doctor` explains and repairs; 100 parallel `ivk new` against one repo produce
100 consistent registry rows; both old (JSON-era) and new repos work.

## Phase C: Libgit2Backend (v0.3.x)

Goal: the same kernel runs where no `git` binary exists.

- [ ] `git2` optional dependency behind feature `libgit2`
- [ ] implement the Phase A trait surface on libgit2
- [ ] add the network half to the trait for both backends:
      `clone_repository`, `fetch`, `checkout`, with `CredentialProvider`
      (HTTPS + personal-access-token first; ssh-agent later)
- [ ] **parity test suite**: every kernel operation runs against both
      backends on the same fixture repo; resulting worktree bytes, status,
      diff, refs are diffed — divergence is a test failure. Desktop `git`
      remains the reference implementation.
- [ ] not implemented (recorded refusals, same style as v2's deferred flags):
      LFS, submodules beyond "present = warn", partial clone, sparse
      checkout, credential helpers beyond token callback, rebase, merge
      tooling
- [ ] `ivk doctor` reports which backend is active and why

Exit criteria: `cargo test --features libgit2` green on macOS + Linux CI;
parity suite covers clone → new → edit → status → diff → ch new → export →
patch → rm → gc end-to-end.

## Phase D: ivk-ffi + XCFramework (v0.4.x)

Goal: CodeOn can link the kernel.

- [ ] `crates/ivk-ffi`: UniFFI bindings over the kernel API (workspace CRUD,
      status, diff, changeset, export/import, doctor) — JSON-envelope
      semantics preserved so agent tooling behaves identically through FFI
- [ ] build targets `aarch64-apple-ios`, `aarch64-apple-ios-sim`,
      `x86_64-apple-ios`; `scripts/build-xcframework.sh`
- [ ] libgit2 + TLS on iOS resolved (SecureTransport / bundled mbedTLS —
      decide with a spike; this is the known-risk item that would trigger the
      gix fallback)
- [ ] materializer decision on-device: Rust `clonefile(2)` vs host-injected
      Swift copy callback (see decision record); measured on a real device
- [ ] CI: build-only iOS job (no simulator tests initially)
- [ ] SQLite on iOS: rusqlite bundled — verify WAL + file-protection classes
      interact sanely with app suspension

Exit criteria: a demo Swift package checks out a GitHub repo over HTTPS,
creates a workspace, edits a file, shows status/diff, and survives app kill —
on simulator and one physical device.

## Phase E: kernel features both frontends need (v0.5.x)

- [ ] snapshot including untracked files (git-native: `git stash create`-like
      tree write without moving HEAD; works on both backends)
- [ ] export/import bundle = patch + untracked files + metadata (move work
      between machines / phone↔desktop without a remote)
- [ ] stale detection: workspace records base commit; doctor/ls flag
      workspaces whose base is no longer the branch head
- [ ] policy enforcement: `agent-policy.toml` `allow_commit` / `allow_push`
      enforced in the kernel (FFI callers cannot bypass); push stays absent
      from the trait until this lands
- [ ] agent-session metadata KV on workspaces (`agent`, `thread_id`,
      `issue_url`, `pr_url`, …) + `ivk ws meta` get/set + Issue/PR
      association surfaced in `ls --json`
- [ ] `ivk ship` (ch new + export + push + PR) — desktop-only convenience,
      gated on the policy layer above (Risk 9 of v2)

## Phase F: frontend enablement (ongoing)

- [ ] CodeOn MVP integration support (ivk side of bug fixes, API gaps)
- [ ] MCP server (`ivk mcp serve`) — the spec has been in `skills/ivk/mcp.md`
      since v2; becomes real once the kernel API is FFI-stable
- [ ] IvyHub design doc refresh against the changeset/export/import model

---

## v2 deferred items — where they landed

| v2 deferred item | v3 home |
|---|---|
| SQLite (`.ivk/db.sqlite`) | Phase B (core deliverable) |
| `ivk ws rm --failed` / `--all-discarded` | Phase B (DB state makes them well-defined) |
| `--from <rev>` | Phase A seam → Phase B CLI wiring |
| tracing | Phase B |
| clap port | when subcommand count next grows (likely with Phase B flags); not a phase gate |
| `ivk ws mount` | unscheduled — revisit if workspaces need to live outside `.ivk/` (CodeOn may force this; decide in Phase D) |
| `ivk ship` | Phase E (behind policy layer) |
| `ivk bench matrix` | unscheduled (unchanged) |
| real-agent orchestration scripts | unscheduled (unchanged) |

## Risks specific to v3

1. **libgit2 behavioral drift vs git CLI** — mitigated by the parity suite
   (Phase C) and by keeping desktop on GitCliBackend, so drift affects only
   platforms that have no alternative anyway.
2. **iOS TLS/cross-compile friction for libgit2** — known-risk spike early in
   Phase D; documented fallback: `gix`.
3. **Registry migration breaking v0.0.x users** — Phase B migration is
   scan-and-backfill (the old layout *is* the data); JSON changeset files are
   kept as artifacts; `ivk doctor` validates DB↔directory agreement.
4. **Scope pull from CodeOn** — the boundary in the decision record is the
   contract: UI, editor, terminal, App Store concerns never enter this repo.
   ivk ships facts through `ivk-ffi`.
5. **GPLv3 (Blink) is CodeOn's risk, not ivk's** — recorded above so licensing
   questions do not block kernel work.
