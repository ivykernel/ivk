# Agent Instructions

This repository uses **Ivy Kernel (`ivk`)** for parallel AI-agent workspaces.

You are an AI coding agent (Claude Code, Codex, Cursor, or similar). Read this file before touching any code.

## Why ivk exists here

The maintainer runs many coding agents in parallel on this repo. Each agent gets its own isolated working tree, materialized cheaply via APFS clonefile (or Linux reflink). The kernel tracks the lifecycle so failed attempts can be discarded without polluting the base checkout.

## Rules

1. **Do not edit files in the repository root.** Instead, create a workspace for your task and work inside it.
2. **Do not run `git worktree add` manually.** Use `ivk new <task-name>`.
3. **Do not push directly to the remote** unless the user explicitly asks. Export through `ivk export` (or, once available, `ivk ship`).
4. **Do not delete `.ivk/`.** It contains workspace lifecycle state.

## Workflow

```bash
# 1. Check current state.
ivk doctor --agent --json

# 2. Create a workspace for your task.
ivk new <short-task-name>

# 3. Move into it.
cd .ivk/workspaces/<short-task-name>

# 4. Make changes. Run tests. Iterate.

# 5. When done, record + export:
ivk ch new <short-task-name>
ivk ch check <ch-id>              # conflict status vs HEAD; conflicts => rebase + ch new again
ivk export <ch-id> agent/<short-task-name>
# (optional) write a unified-diff patch file:
ivk patch <ch-id>
# (coming) all-in-one: changeset + export + push + gh pr create
# ivk ship <short-task-name>

# 6. If the attempt failed, discard:
ivk ws rm <short-task-name>

# 7. End-of-batch cleanup:
ivk ws rm --exported --yes              # discard workspaces already preserved on agent/<ws> branches
ivk gc                                  # prune orphan workspace dirs + git worktree admin entries; reports bytes_reclaimed
```

Use bash brace expansion for parallel candidates:

```bash
ivk new attempt-{1,2,3}
```

`ivk gc` and bulk `ivk ws rm --all/--exported` will NEVER delete a workspace with uncommitted edits unless you pass `--force`, and will NEVER touch `.ivk/changesets/*.json`. Run with `--dry-run` first when in doubt.

## When you don't know what to do

```bash
ivk doctor --agent --json
```

The response includes a `next_command` field. Run it. Re-check. Repeat.

## Further reading

- [`skills/ivk/SKILL.md`](./skills/ivk/SKILL.md) — full skill manifest.
- [`skills/ivk/cli.md`](./skills/ivk/cli.md) — per-command reference.
- [`skills/ivk/workflow.md`](./skills/ivk/workflow.md) — complete workflows (single task, multi-attempt, multi-agent).
- [`skills/ivk/rules/safety.md`](./skills/ivk/rules/safety.md) — exhaustive safety rules and known footguns.
