# Recorded agent session

A real walk-through of the [`demo-task`](./README.md) fixture. Every JSON
block below is the *literal* output captured from a fresh setup; no
hand-editing. This is what an agent that follows the golden path should see.

The agent is assumed to start at the demo-task repo root with no prior
context. Its first action is always `ivk doctor --agent --json`.

---

## Step 1 — Discover state

```bash
$ ivk doctor --agent --json
```

```json
{
  "ok": true,
  "command": "doctor",
  "next_command": "ivk new <task-name>",
  "recommended_next_steps": [
    "You are not inside an ivk workspace yet.",
    "Create one for the current task: `ivk new <task-name>`.",
    "Then `cd .ivk/workspaces/<task-name>` and re-run `ivk doctor --agent --json`."
  ],
  "repo_initialized": true,
  "inside_ivk_workspace": false,
  "ivk_dir_present": true,
  "has_changes": false,
  "repo_root": "/.../demo-task",
  "strategy": "apfs-clonefile"
}
```

**Decision:** the agent is at the repo root, ivk is set up, no workspace yet.
Follow `next_command`. Pick a short task name; here we use `fix-sumto`.

---

## Step 2 — Create the workspace

```bash
$ ivk new fix-sumto --agent --json
```

```json
{
  "ok": true,
  "command": "ws.new",
  "next_command": "cd ./.ivk/workspaces/fix-sumto && ivk doctor --agent --json",
  "recommended_next_steps": [
    "cd ./.ivk/workspaces/fix-sumto to work in the new workspace.",
    "Run tests / make edits inside the workspace.",
    "Run `ivk doctor --agent --json` when unsure of next step."
  ],
  "created": [
    {
      "name": "fix-sumto",
      "path": "./.ivk/workspaces/fix-sumto",
      "entries_cloned": 5,
      "elapsed_ms": 56,
      "strategy": "apfs-clonefile"
    }
  ],
  "failed": []
}
```

**Decision:** workspace materialized in 56 ms. Follow `next_command`.

---

## Step 3 — Verify, then edit

```bash
$ cd .ivk/workspaces/fix-sumto
$ node --test test/*.test.js
# tests 4
# pass 1
# fail 3      ← bug confirmed
```

Read `src/sum.js`. The loop is `i < n`; it should be `i <= n`. Apply the fix
to **the workspace's copy**, not the base repo:

```bash
$ sed -i '' 's|i < n|i <= n|' src/sum.js
$ node --test test/*.test.js
# tests 4
# pass 4
# fail 0      ← all four pass
```

---

## Step 4 — Doctor sees the dirty workspace

```bash
$ ivk doctor --agent --json
```

```json
{
  "ok": true,
  "command": "doctor",
  "next_command": "ivk ch new fix-sumto — once tests pass, record this as a changeset",
  "recommended_next_steps": [
    "Run project tests inside this workspace.",
    "If tests pass, record a changeset: `ivk ch new fix-sumto`.",
    "If the attempt failed, discard the workspace: `ivk ws rm fix-sumto`."
  ],
  "repo_initialized": true,
  "inside_ivk_workspace": true,
  "ivk_dir_present": false,
  "workspace_name": "fix-sumto",
  "workspace_status": "dirty",
  "has_changes": true,
  "repo_root": "/.../demo-task/.ivk/workspaces/fix-sumto",
  "strategy": "apfs-clonefile"
}
```

**Decision:** tests pass, follow `next_command`. From the workspace dir we
can refer to the workspace by name from anywhere in the repo:

---

## Step 5 — Record the changeset

```bash
$ cd /.../demo-task           # back to repo root
$ ivk ch new fix-sumto --agent --json
```

```json
{
  "ok": true,
  "command": "ch.new",
  "next_command": "ivk export ch_0909b3fba5a2 agent/fix-sumto",
  "recommended_next_steps": [
    "Changeset ch_0909b3fba5a2 created from workspace fix-sumto.",
    "Export to a Git branch: `ivk export ch_0909b3fba5a2 agent/fix-sumto`."
  ],
  "id": "ch_0909b3fba5a2",
  "workspace_name": "fix-sumto",
  "base_snapshot": "1ba6a17f4a776fb96d79ff12e80ef88ff87b06cf",
  "result_snapshot": "0909b3fba5a2a44a9625fe032bb2c3cb1c8e2022",
  "touched_paths": ["src/sum.js"],
  "created_at_unix": 1782403017,
  "files_changed": 1,
  "insertions": 1,
  "deletions": 1
}
```

**Decision:** changeset recorded as `ch_0909b3fba5a2`. The diff is exactly
what we expected: one file, +1/-1. Follow `next_command`.

---

## Step 6 — Export to a Git branch

```bash
$ ivk export ch_0909b3fba5a2 --agent --json
```

```json
{
  "ok": true,
  "command": "export",
  "next_command": "git push origin agent/fix-sumto",
  "recommended_next_steps": [
    "Branch `agent/fix-sumto` now points at 0909b3fba5a2.",
    "Push and open a PR: `git push origin agent/fix-sumto && gh pr create`."
  ],
  "changeset_id": "ch_0909b3fba5a2",
  "branch": "agent/fix-sumto",
  "sha": "0909b3fba5a2a44a9625fe032bb2c3cb1c8e2022"
}
```

**Decision:** `agent/fix-sumto` now points at the fix. If the user wants the
PR opened, run `git push origin agent/fix-sumto && gh pr create`. If they
want to keep the work local for further review, stop here.

---

## What this transcript verifies (Phase 6 exit criteria)

- [x] **Agent uses the ivk workflow without manual explanation** — every step
      was driven by `next_command` and `recommended_next_steps` from the
      previous JSON output.
- [x] **Agent does not create a raw `git worktree`** — only `ivk new` was used.
- [x] **Agent does not edit the base repo** — `src/sum.js` was modified only
      inside `.ivk/workspaces/fix-sumto/`.
- [x] **Agent creates a changeset and exports through ivk** — `ivk ch new`
      followed by `ivk export`.

Reproduce locally:

```bash
bash examples/demo-task/setup.sh
cd .cache/demo-task
# follow the steps above
```
