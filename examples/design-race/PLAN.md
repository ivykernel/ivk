# Demo plan: 4 design variants in parallel

Recording recipe for: **four versions of the same web app running
side-by-side in the browser**, each in its own ivk workspace, each running
its own dev server, all hot-reloaded independently.

**Honest framing**: at N=4, `git worktree` does this too. The recording is
a *parallel-development tutorial*, not an ivk-only capability. The
differences appear with scale (10+ workspaces) and frequency (do this five
times a day). See [§ "Where ivk earns its keep here"](#where-ivk-earns-its-keep-here)
below for the calibrated comparison.

The browser side is screen-captured, not vhs-able.

---

## Prerequisites

```text
- macOS (Apple Silicon recommended for clonefile demo)
- Node 18+ in PATH  (for `node --test`)
- pnpm 8+           (for the install)
- Google Chrome     (or any browser; layout commands below assume Chrome)
- ivk binary built  (cargo build --release --workspace)
- demos/build-race.tape's Vite fixture: .cache/fixture-vite/
  (run examples/demo-task/setup.sh OR scripts/bench/build/setup-vite.sh once)
```

Workspace approach: each design variant gets its own ivk workspace at
`.ivk/workspaces/design-<x>/`. ivk creates them in ~150 ms each, so the
"spin up four workspaces" step is sub-second total. The clonefile share
means the four `node_modules/` directories are block-shared from the
source — no extra disk for deps.

---

## Setup (~2 min, one-time)

```bash
# 1. Generate / refresh the Vite fixture.
bash scripts/bench/build/setup-vite.sh

# 2. cd into the fixture and create four workspaces.
cd .cache/fixture-vite
ivk init                            # creates .ivk/
ivk new design-{a,b,c,d}            # one bash brace expansion, four ws
                                    # ~600 ms total via clonefile

# 3. Verify all four are clean git worktrees.
ivk ls
# Expected: 4 workspaces, all "clean"

# 4. Each workspace's node_modules is already populated (cloned from
#    fixture-vite). Confirm one dev server boots cleanly before launching all four.
(cd .ivk/workspaces/design-a && pnpm dev --port 5173 &)
# Open http://localhost:5173 → should see the unedited fixture
kill %1
```

---

## The four variants — choose distinct visual changes

Pick changes that are **visually obvious in a side-by-side screenshot**.
The point is to demonstrate parallel exploration, not subtle A/B tests.

| ws | edit | files touched |
|---|---|---|
| **design-a** | original baseline (no edit) | none |
| **design-b** | dark theme: change body bg + text color in `src/App.tsx` | 1 |
| **design-c** | bigger hero: bump h1 font-size + add badge | 1 |
| **design-d** | new layout: stack vertical → horizontal grid via flexbox | 1-2 |

Concrete sed-able edits live in `examples/design-race/edits.sh` (sibling
script). Each is a single-line `sed -i` so the demo viewer sees the file
mutation happen live in the terminal panel.

---

## Launch the four dev servers (parallel)

Each workspace needs its own port. Vite defaults to 5173 and auto-bumps,
but explicit ports make the layout deterministic.

```bash
# In four separate terminals (or one with `tmux`):
(cd .ivk/workspaces/design-a && pnpm dev --port 5173 --strictPort) &
(cd .ivk/workspaces/design-b && pnpm dev --port 5174 --strictPort) &
(cd .ivk/workspaces/design-c && pnpm dev --port 5175 --strictPort) &
(cd .ivk/workspaces/design-d && pnpm dev --port 5176 --strictPort) &
```

The four `pnpm dev` processes share the global pnpm store automatically;
the only per-process cost is the Vite dev server's RAM (~400 MB each, so
total ~1.6 GB — easy on any modern Mac).

---

## Browser layout (Chrome, four-up)

```bash
# Open all four URLs in separate browser windows for tiling.
for port in 5173 5174 5175 5176; do
  /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
    --new-window --window-size=900,600 "http://localhost:${port}"
done
```

Then arrange manually with macOS Mission Control or Rectangle / Magnet
window tiler into a 2×2 grid. (Stage Manager works too.)

For Recording — Chrome has `--app=URL` mode that hides the toolbar:

```bash
for i in a b c d; do
  case $i in a) p=5173;; b) p=5174;; c) p=5175;; d) p=5176;; esac
  /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome \
    --app="http://localhost:${p}" --window-size=900,600 &
done
```

Chrome app windows are cleaner-looking for a recording.

---

## Recording approach

```text
Tool:  QuickTime Player (Cmd-Shift-5) — built-in, no install
Area:  full-screen (so all four browser quadrants visible)
Mic:   off (no narration; add subtitles or voice-over later)
Sound: off
Length goal: 60 seconds
Output:  examples/design-race/recording.mov  (then convert to .mp4)
```

Beat-by-beat (for the recording, ~60s):

1. **0:00–0:08** — Terminal pane fills the screen. Type `ivk new design-{a,b,c,d}`,
   show the JSON output noting all four created in <1 s.
2. **0:08–0:12** — Quick `ls .ivk/workspaces/` to show the four directories exist.
3. **0:12–0:18** — Launch four `pnpm dev` in background (script).
4. **0:18–0:25** — Switch to four-up Chrome layout. All four windows show the
   identical (baseline) app.
5. **0:25–0:45** — Click into the terminal again, apply each edit
   (`bash examples/design-race/edits.sh`). HMR reloads each browser quadrant
   live; the four windows visually diverge.
6. **0:45–0:55** — Highlight one window as "the winner". `ivk ch new design-c`
   in the terminal, then `ivk export <ch-id> agent/new-design`.
7. **0:55–1:00** — `ivk rm design-{a,b,d}` to discard. Disk usage one-liner
   (`du -sh .ivk/workspaces`) to show only the winner's bytes remain.

---

## Cleanup

```bash
# Kill all four dev servers
pkill -f "pnpm.*dev"

# Discard the workspaces
ivk rm design-a design-b design-c design-d 2>/dev/null
# (or `ivk ws rm --all --yes --force` to drop everything)
```

---

## Where ivk earns its keep here

`git worktree` can do this same 4-workspace setup. The differences scale
with N and with iteration count, not with one-shot use.

```text
                       git worktree           ivk
N=4 (this demo)
  disk                 2.8 GB                 40 MB           ratio 70×
  setup time           4–10 s (× pnpm install) 600 ms          ratio 7–15×
  RAM (4 × Vite dev)   ~1.6 GB                ~1.6 GB         same
  cleanup              shell loop you write   `ivk rm a b c d`  ergonomics

N=10 (still ergonomic)
  disk                 7 GB                   ~80 MB          ratio ~90×
  setup time           10–30 s                1.5 s           ratio ~20×
  pain                 "laptop's getting hot" "imperceptible"

N=100 (where git worktree breaks)
  disk                 70 GB                  ~400 MB         ratio ~175×
  setup time           1.5–5 min              ~24 s           ratio ~5–12×
  pain                 disk full, manual GC   ✓ works
```

The RAM cost dominates either way (the four Vite dev servers); ivk's edge
is on disk and lifecycle, not RAM. The recording should make the disk
delta visible (a brief `du -sh` overlay at the end) but should not claim
a RAM win that doesn't exist.

So this demo is **a parallel-development tutorial that ivk happens to make
ergonomic at scale**. For a more pointed "ivk-only" demo, see
[`examples/agent-fanout/PLAN.md`](../agent-fanout/PLAN.md) — the same
fixture but with 10–30 workspaces orchestrated by an actual agent (Claude
Code / Codex / Cursor) where the git worktree path is genuinely painful.

---

## Out of scope for v0.0.x

- Auto-port allocation by `ivk new` (planned: `ivk new --auto-port` injects
  `PORT` env var). Currently the user supplies ports manually.
- A tiling helper for browser windows (currently macOS Mission Control or
  Rectangle).
- Sharing the recording: the user uploads to GitHub Releases or to the LP
  manually. No `ivk demo upload` command planned.
