# Demo plan: agent fan-out (10–30 workspaces)

The case where `git worktree` becomes genuinely impractical. Pitches
ivk's *scaling* axis and its *agent-native* JSON output, both of which
matter less at N=4 (see [`../design-race/PLAN.md`](../design-race/PLAN.md))
and decisively at N≥10.

---

## The scenario

A single user wants to ask three AI coding agents — Claude Code, Codex CLI,
Cursor — to attempt the same bug, each with N attempts (N=4, 5, or 10
depending on patience). Total: **12 to 30 simultaneous workspaces**.

The user's machine is a developer laptop, not a beefy cloud box.

```
Goal:
  Fix the off-by-one in examples/demo-task/src/sum.js.
  Each agent gets N attempts to converge.
  At the end, show which attempts passed `node --test`, pick the cleanest
  winning diff, export it, discard the rest.
```

---

## Why git worktree is painful here

```text
12 worktrees of fixture-vite (700 MB each, deps included)
   = 8.4 GB on disk           on a 256 GB laptop, gone in a minute
30 worktrees                  = 21 GB                       — uncomfortable
30 × git worktree add         = 30–90 seconds setup        — feels slow
manual cleanup                = bookkeeping you write yourself
no JSON output                = the agent has to parse `git worktree list`
                                output to know what's there
```

The pnpm store sharing keeps `node_modules` mostly inexpensive, but the
working tree and any `dist/` / `.next/` artifacts still grow linearly.
By 30 workspaces you're paying real money in disk.

---

## With ivk

```text
30 ivk workspaces
   = ~120 MB on disk via clonefile          (~175× less)
   = ~3 s to materialize all 30             (vs 60-90 s)
   = `ivk new claude-1 claude-2 ... cursor-10`  one shell call
   = `ivk ls --json` for status              machine-parseable
   = `ivk doctor --agent --json` per ws      next_command for the orchestrator
```

The agents don't need to know about clonefile; they just `cd
.ivk/workspaces/claude-3` and work as if they were the only one.

---

## Setup

```bash
# 1. Generate the demo-task fixture (off-by-one bug + failing tests).
bash examples/demo-task/setup.sh
cd .cache/demo-task

# 2. Create 12 to 30 workspaces. Single command, sub-second:
ivk new \
  claude-{1,2,3,4} \
  codex-{1,2,3,4} \
  cursor-{1,2,3,4}

# 3. Sanity check.
ivk ls
# → 12 workspaces, all "clean"
du -sh .ivk/workspaces
# → ~50 MB total (clonefile)
```

---

## Orchestration: how each agent gets its workspace

This part assumes one of the supported coding agents (Claude Code, Codex
CLI, Cursor) reads `AGENTS.md` and `skills/ivk/` from the repo, which
`ivk init --agent-instructions` drops in place. The agent's workflow:

```bash
# Inside an agent's tool-use loop (one iteration per attempt):
NAME="$AGENT_NAME-$ATTEMPT_N"             # e.g. "claude-3"
ivk doctor --agent --json                  # discover state
cd ".ivk/workspaces/$NAME"                  # enter assigned workspace
# ... agent edits files, runs node --test ...
if tests_passed; then
  cd ..
  ivk ch new "$NAME"                       # snapshot
  ivk export "$ch_id"                      # creates agent/$NAME branch
else
  cd ..
  ivk rm "$NAME"                           # discard, don't pollute
fi
```

For a recorded demo, we *simulate* the agents with the same harness as
`examples/todo-100/simulate.sh`: a deterministic script that creates each
workspace, applies a canned fix (or a canned broken edit), and records
the outcome. The point of the recording is the *bookkeeping at scale*,
not whether a real LLM converged.

---

## The recording (~90 seconds)

```text
Tool:       QuickTime full-screen + iTerm split panes (3 across, top → bottom)
Output:     examples/agent-fanout/recording.mov (then .mp4 for LP)
Resolution: 1920×1200 or so; vary for retina
```

Beat-by-beat:

| t       | beat |
|---------|------|
| 0:00    | Empty `.cache/demo-task/` shown in left pane. `ivk init` runs. |
| 0:04    | `ivk new claude-{1..4} codex-{1..4} cursor-{1..4}` — 12 workspaces materialize in ~600 ms. |
| 0:08    | `du -sh .ivk/workspaces` shows ~50 MB. Cut to right pane: a similar `git worktree add` loop runs, takes ~25 s and pumps disk to 8 GB. |
| 0:35    | Back to ivk pane. `bash examples/todo-100/simulate.sh --pass 6 --fail 6` adapted to use the 12-name set. The simulator works through each workspace serially. |
| 0:60    | `ivk ls --json` rendered with `jq` to a colored table: 6 passed (have branches), 6 failed (already removed). |
| 0:75    | `git branch | grep agent/` shows the 6 winners. Pick the cleanest diff visually. |
| 0:85    | `ivk gc --yes` cleans any orphaned admin state. `du -sh .ivk/workspaces` → near zero. |
| 0:90    | End card: "12 attempts. 6 winners. 50 MB. No bookkeeping." |

---

## Variants

```text
small  — 12 workspaces (3 agents × 4 attempts).   60–90 s recording.
medium — 30 workspaces (3 agents × 10 attempts).  3 min recording.
large  — 100 workspaces (per `examples/todo-100/`). Already exists.
```

Use the **small** variant for the LP and Twitter (90 s fits in the
mid-scroll attention window). Use **large** when responding to "does it
really scale?" questions; `examples/todo-100/simulate.sh` is already
runnable end-to-end with measured numbers.

---

## Why this demo lands

- **N=12 onwards, `git worktree` is genuinely uncomfortable**: 8 GB disk,
  30-second setup, manual cleanup of failed attempts. The viewer feels
  it.
- **The agent's golden path is visible**: `ivk doctor --agent --json`
  drives every step; the orchestrator is mechanical.
- **Failure is cheap and visible**: `ivk rm` happens *during* the demo,
  not as a manual postscript. Watching half the attempts get discarded
  is part of the pitch.

This is the demo to lead with when the audience is "developers running
multiple AI agents", not "developers running one agent".

---

## Out of scope for v0.0.x

- Actual real-agent orchestration (would need three CLIs configured with
  the same tool-use loop and a real bug). Use the simulator until we
  have it.
- Per-agent metrics dashboard (which agent succeeded most often, etc.).
- Auto-discovery of a free port range for any per-workspace dev servers
  (planned: `ivk new --auto-port`).
