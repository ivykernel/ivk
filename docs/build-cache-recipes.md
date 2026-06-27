# Build cache recipes for ivk

"My AI agent spins up N worktrees and each one starts with cold caches"
is a real problem with `git worktree`. This doc covers, per toolchain,
**what ivk handles for free** and **what one extra config line buys you**.

## The three layers, in one diagram

```text
                  what ivk gives you             what one env var adds            what's still on you
────────────────────────────────────────────────────────────────────────────────────────────────────────
Cold cache        clonefile inherits source's    —                               —
                  warm caches into every new ws
─────────────────
Edit-and-rebuild  fast incremental in *this* ws  point CARGO_TARGET_DIR /        —
across N ws       (cache survives clonefile)     TURBO_REMOTE_CACHE_HOME /
                                                 etc. at one shared dir
─────────────────
Remote / team     —                              —                               sccache, Turbo remote,
cache                                                                            Bazel remote, etc.
```

If you only have one workspace, layer 1 alone is enough. If your AI agent
spawns 10+ workspaces *and* they each modify code and rebuild, you want
layer 2. Layer 3 is the cross-machine / cross-CI story that ivk doesn't
try to be.

---

## Cargo (Rust) ★ best case

**Layer 1 — automatic**: clonefile preserves `target/.fingerprint/`. The
new workspace's cargo sees its cache as valid and does nothing on first
build. **200× faster** than git worktree's cold rebuild ([spike numbers](../results/build-summary.md#rust--cargo-even-more-dramatic)).

```bash
ivk new fix-auth
cd .ivk/workspaces/fix-auth
cargo build --release      # 0.3s (cache HIT) — git worktree would be 37s
```

**Layer 2 — one env var**: point all workspaces at a shared target dir
so incremental rebuilds after edits land in one place instead of N. Use
this when agents actually modify source.

```bash
# Put this in your shell rc, or have ivk inject it (planned: `ivk new --tool cargo`)
export CARGO_TARGET_DIR="$HOME/.cache/ivk/$(basename $PWD)/cargo-target"
```

Cargo locks the target dir during compilation, so concurrent builds
serialize cleanly without corruption.

**Caveat**: incremental cache invalidation depends on cargo's
fingerprint logic. Changing absolute paths (e.g. moving a workspace)
invalidates. Inside a stable `.ivk/workspaces/<name>/` location, fine.

---

## pnpm (TypeScript / JavaScript) ★ best case

**Layer 1 — automatic**: pnpm already uses a global content-addressed
store at `~/.pnpm-store` regardless of ivk. Every workspace's
`node_modules/` is symlinks into it. clonefile preserves the symlink
tree so even the per-workspace `node_modules/` is metadata-only after
ivk new.

```bash
ivk new design-a
cd .ivk/workspaces/design-a
pnpm install --frozen-lockfile     # ~3s (refreshes symlinks from store)
pnpm dev                            # works immediately
```

Disk cost per workspace: a few MB of symlinks + lockfile, even for a
1000-dep project.

**Layer 2 — env var (rarely needed)**: pnpm's store is already shared
across all workspaces of all projects by default. No additional config
needed unless you want a per-repo store.

```bash
# Only if you want isolation between repos:
echo "store-dir=$HOME/.cache/ivk/$(basename $PWD)/pnpm-store" >> .npmrc
```

**Caveat**: `pnpm install` still needs to run if the lockfile differs
between workspaces (most agent flows don't change the lockfile).

---

## npm / yarn classic (TypeScript / JavaScript) ★ where ivk earns its keep

These don't have a global content store — each project's `node_modules/`
is a flat tree of real files. Without ivk, 10 git worktrees = 10×
`node_modules/`.

**Layer 1 — automatic and significant**: clonefile makes the per-workspace
`node_modules/` block-shared. 10 workspaces = ~1× the disk cost of one.

```bash
ivk new bugfix-1
cd .ivk/workspaces/bugfix-1
# node_modules is already there (clonefile'd); no install needed
npm test                            # uses the inherited node_modules
```

**Layer 2 — switch to pnpm**: long-term, migrating off flat
node_modules is the better fix. ivk + pnpm is strictly better than ivk +
npm.

**Caveat**: when a workspace runs `npm install` and modifies
`node_modules/`, clonefile-shared blocks break and the workspace gets
its own copy. That's per-block CoW, not whole-tree, so the cost is
proportional to what changed.

---

## TypeScript compiler (`tsc -b` / project references)

**Layer 1 — automatic**: `tsbuildinfo` files use content hashes and
relative paths. clonefile-inherited tsbuildinfo stays valid in the new
workspace. The first `tsc -b` in a workspace skips files that haven't
changed.

**Layer 2 — env var**: `TSC_COMPILE_ON_ERROR` and friends don't apply
here. The cache is per-project; nothing to share across workspaces.

**Caveat**: if you edit a file, tsc invalidates only its dependents.
Same as single-workspace behavior.

---

## Vite / Next.js / esbuild build output (`dist/`, `.next/`, `.vite/`)

**Layer 1 — partial**: clonefile inherits the source's last-built
`dist/` (or `.next/`). On the workspace's next `pnpm build`, the
bundler will re-emit `dist/` and the clonefile sharing for those files
breaks (per-block CoW). End state: each workspace has its own `dist/`.

Numbers from the [TS spike](../results/build-summary.md#ts--vite-n--50-typescript--vite-typical):

```text
N=50 workspaces, each runs pnpm build
  git worktree post-clone disk:    3.43 GB
  ivk     post-clone disk:    93 MB         (37x cheaper before build)
  git worktree post-rebuild disk:  4.01 GB
  ivk     post-rebuild disk:  500 MB        (8x cheaper after build)
```

The post-build delta (407 MB across 50 workspaces ≈ 8 MB per
workspace) is the per-workspace `dist/` that ivk doesn't share.

**Layer 2 — Turborepo / Nx local cache**: if your repo uses Turbo or
Nx, point them at a shared local cache.

```bash
export TURBO_REMOTE_CACHE_HOME="$HOME/.cache/ivk/$(basename $PWD)/turbo"
# Turbo automatically deduplicates build outputs by hash
```

**Caveat**: Vite's `.vite/` dev cache works fine inherited. Next.js's
`.next/` is more state-heavy; some incremental cache may invalidate on
first dev-server start in a fresh workspace.

---

## uv / pip (Python)

**Layer 1 — automatic for uv**: uv uses a global cache at `~/.cache/uv`
(or `$UV_CACHE_DIR`) by default. clonefile makes the per-workspace
`.venv/` symlinks block-shared. Fast inheritance.

```bash
ivk new analysis-1
cd .ivk/workspaces/analysis-1
uv sync       # near-instant if lockfile unchanged
```

**Layer 2 — env var**: `UV_CACHE_DIR` already shared by default; no
change needed unless you want per-repo isolation.

**For pip**: same shape as npm classic. clonefile inherits `.venv/`,
but `pip install` will write per-workspace if invoked.

---

## Go

**Layer 1 — automatic**: Go uses two caches:
- `GOCACHE` (default `~/Library/Caches/go-build` on macOS) for compiled artifacts
- `GOMODCACHE` (default `~/go/pkg/mod`) for module sources

Both already global-shared regardless of worktree. clonefile gives you
fast workspace materialization on top; cache inheritance is automatic.

**Layer 2 — nothing to do**: Go's cache discipline is excellent. ivk
doesn't add anything beyond cheap worktrees.

---

## Gradle (Java / Kotlin / Android)

**Layer 1 — partial**: Gradle's user home (`~/.gradle/`) is global by
default. The per-project `.gradle/` is per-workspace. clonefile inherits
both.

**Layer 2 — env var**: `GRADLE_USER_HOME` is already global; no change
needed. For build cache, enable Gradle's local build cache:

```groovy
// settings.gradle.kts
buildCache {
  local {
    directory = file("$rootDir/.ivk/gradle-cache")   // shared across workspaces
  }
}
```

**Caveat**: Gradle's daemon is heavyweight; running 10 simultaneous
daemons across 10 workspaces costs RAM. Out of scope for ivk.

---

## Bazel / Buck (large-scale)

ivk + Bazel = use Bazel's `--disk_cache=` pointing at a shared dir.
`build --disk_cache=$HOME/.cache/ivk/bazel`. Bazel's remote cache /
remote execution are the orthogonal scaling story and not something ivk
tries to replace.

---

## sccache (cross-toolchain compiler cache)

If you already run sccache as your compiler proxy, ivk doesn't change
anything: sccache's cache is a single directory shared globally. ivk
just lets you have more parallel cargo/clang invocations all using the
same sccache.

```bash
export SCCACHE_DIR="$HOME/.cache/sccache"
export RUSTC_WRAPPER=sccache
# Now every workspace's cargo build hits the same shared compile cache.
```

---

## How to decide which layer you need

```text
You have:
  1 workspace, run cargo / pnpm sometimes        → layer 1. you're done.
  2-5 workspaces, mostly read/look-only          → layer 1.
  2-5 workspaces, frequent edits + rebuilds      → layer 1 + cargo target dir
  10+ workspaces (agents), each running builds   → layer 2 for your toolchain
  Cross-machine / CI cache sharing               → layer 3 (sccache, Turbo,
                                                   Bazel, etc. — outside ivk)
```

---

## Future: `ivk new --tool <name>` auto-injection

Planned (deferred from v0.0.x): `ivk new fix-auth --tool cargo` would
inject the right `CARGO_TARGET_DIR` env into the workspace's shell
config so layer 2 happens automatically. Same for `--tool turbo`,
`--tool uv`, etc.

For now, set the env vars yourself or in your shell rc. The recipes
above are stable and won't change shape when the flag lands.

---

## See also

- [`results/build-summary.md`](../results/build-summary.md) — the spike numbers behind these recipes
- [`docs/portability.md`](portability.md) — what reflink primitives exist on each OS
- [`AGENTS.md`](../AGENTS.md) — what to drop in your repo so AI agents follow these recipes too
