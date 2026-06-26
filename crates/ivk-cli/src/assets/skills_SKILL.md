---
name: ivk
description: Manage Ivy Kernel workspaces for parallel AI-agent development.
user-invocable: false
allowed-tools: Bash(ivk *)
---

# Ivy Kernel Skill

Use Ivy Kernel (`ivk`) in any repository that contains `.ivk/`, an `AGENTS.md` mentioning ivk, or a `skills/ivk/` directory.

`ivk` is a workspace kernel for AI agents. It lets you (the agent) work in an isolated, lifecycle-managed copy of the repository instead of editing the base checkout. Workspaces are cheap to create and discard because the working tree is block-shared with the source via APFS clonefile (on macOS) or filesystem reflinks (on Linux btrfs/xfs/zfs).

## When to use ivk

Always, in any repo that has it set up. Specifically:

- Before any non-trivial edit, create a workspace for the task and work inside it.
- If you have multiple candidate approaches, create one workspace per candidate (`ivk new attempt-a attempt-b attempt-c`) and compare results.
- If the first attempt fails, discard the workspace and start a new one rather than fighting the dirty state.

## Discover current state first

Always inspect with:

```bash
ivk doctor --agent --json
```

The JSON tells you:

- `repo_initialized`     — is there a git repo here?
- `inside_ivk_workspace` — are you already inside an ivk workspace?
- `workspace_name`       — if so, its name
- `workspace_status`     — `clean` or `dirty`
- `next_command`         — what to run next
- `recommended_next_steps` — short imperative list

Follow `next_command` unless you have a specific reason not to.

## Golden path

```bash
# 1. From the repo root: create a workspace per task.
ivk new fix-login

# 2. Work inside the workspace.
cd .ivk/workspaces/fix-login
#   ... edit files, run tests, etc.

# 3. When the change is good:
ivk ch new fix-login                  # snapshot the change as a changeset
ivk export <ch-id> agent/fix-login    # point a git branch at the result commit
ivk patch  <ch-id>                    # (optional) emit a unified-diff .patch file
# Coming: ivk ship fix-login          # all-in-one: changeset + export + push + open PR

# 4. When the attempt failed:
ivk ws rm fix-login
```

## Critical rules

- **Do not edit the base repository directly.** Always create a workspace first.
- **Do not create manual git worktrees** (`git worktree add ...`). Use `ivk new` so ivk can track lifecycle and apply the cheap-clone primitive.
- **Do not run `git checkout -b <branch>`** for task isolation. ivk workspaces ARE the isolation.
- **Do not delete `.ivk/`.** It contains the workspace registry and per-workspace admin state.
- **Do not push directly to git from a workspace** unless the user asked you to. Use `ivk export` (or `ivk ship`, once available).
- **When unsure, run `ivk doctor --agent --json` and follow `next_command`.**

## JSON output convention

Every machine-readable command produces an object with at least:

```json
{
  "ok": true,
  "command": "ws.new",
  "next_command": "cd ./.ivk/workspaces/fix-login && ivk doctor --agent --json",
  "recommended_next_steps": ["...", "..."],
  "error": null,
  "...command-specific payload..."
}
```

On failure, `ok: false`, `error: { code, message }`, and `next_command` points to the recovery action.

## Companion files

- [`skills/ivk/cli.md`](./cli.md) — per-command reference (arguments, flags, JSON shape).
- [`skills/ivk/workflow.md`](./workflow.md) — complete agent workflows (single-task, multi-attempt, parallel agents).
- [`skills/ivk/mcp.md`](./mcp.md) — planned MCP interface (not implemented yet).
- [`skills/ivk/rules/safety.md`](./rules/safety.md) — exhaustive safety rules and known footguns.
