# Demo task — the canonical Phase 6 fixture

A tiny repo plus a single broken function. Used to demonstrate that an AI agent
can walk the `ivk` golden path *without prior knowledge* of the tool, using
only what's in `AGENTS.md`, `skills/ivk/`, and the `--agent --json` output of
each command.

## Setup

This directory ships a `setup.sh` that builds a fresh git repo under
`.cache/demo-task/`, populates it with the buggy code + a failing test, and
runs `ivk init --agent-instructions` so the agent has the skill files
available locally.

```bash
bash examples/demo-task/setup.sh
cd .cache/demo-task
```

## The task

`src/sum.ts` has an off-by-one bug. `npm test` (well, `node --test`) reveals it.

```ts
// src/sum.ts (buggy)
export function sumTo(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {   // bug: should be i <= n
    total += i;
  }
  return total;
}
```

The expected agent transcript is in [`session.md`](./session.md) — every
`ivk` command, its JSON output, and the agent decision at each step.

## Pass / fail criteria for the demo

Read the agent skill files first. Run `ivk doctor --agent --json` *before*
touching anything. Then:

1. **PASS** — agent ends with:
   - a passing `node --test` inside `.ivk/workspaces/<name>/`,
   - a changeset recorded via `ivk ch new`,
   - a branch `agent/<name>` created via `ivk export`,
   - no edits to `src/` in the base repo.
2. **FAIL** if the agent edits `.cache/demo-task/src/sum.ts` directly (outside
   a workspace) or runs `git worktree add` manually.
