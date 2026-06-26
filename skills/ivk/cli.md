# ivk CLI reference (agent-facing)

Every command below supports `--json` for machine output and `--agent` for the agent-friendly form (adds `recommended_next_steps`).

## `ivk --version`

Prints the binary version (e.g. `ivk 0.0.1`). No JSON.

## `ivk help [--agent]`

Prints the golden path. With `--agent` returns the workflow as JSON with `next_command` pointing at `ivk doctor --agent --json`.

## `ivk doctor [--agent] [--json]`

The "git status" for ivk. Returns current state of the cwd:

```json
{
  "ok": true,
  "command": "doctor",
  "next_command": "ivk new <task-name>",
  "recommended_next_steps": [ "..." ],
  "repo_initialized": true,
  "inside_ivk_workspace": false,
  "ivk_dir_present": false,
  "workspace_name": null,
  "workspace_status": null,
  "has_changes": false,
  "repo_root": "/abs/path/to/repo",
  "strategy": "apfs-clonefile"
}
```

When inside a workspace, `workspace_name` and `workspace_status` (`clean` | `dirty`) are populated. Recovery hints land in `next_command` and `recommended_next_steps`.

## `ivk new <name> [<name>...] [--json] [--agent]`

Equivalent to `ivk ws new`. Creates one or more workspaces under `.ivk/workspaces/<name>/`.

Output JSON shape:

```json
{
  "ok": true,
  "command": "ws.new",
  "next_command": "cd ./.ivk/workspaces/<first-name> && ivk doctor --agent --json",
  "created": [
    {
      "name": "...",
      "path": "./.ivk/workspaces/...",
      "entries_cloned": 11,
      "elapsed_ms": 123,
      "strategy": "apfs-clonefile"
    }
  ],
  "failed": []
}
```

Multiple names are supported, including bash brace expansion:

```bash
ivk new attempt-{1,2,3}            # creates 3 workspaces in one call
ivk new attempt-1 attempt-2        # equivalent
```

Common errors:

| code | meaning | recovery |
|---|---|---|
| `not_a_git_repo`   | cwd has no `.git/` | `git init` |
| `missing_argument` | no name given      | pass at least one name |

## `ivk ws new <name>...` (same as `ivk new`)

Fully qualified form. Use this in scripts where ambiguity matters.

## Exit codes

- `0` — success
- `1` — runtime error (file system, git invocation, etc.)
- `2` — usage error (bad arguments)

## Phase 1+ (planned, not yet implemented)

| command | summary |
|---|---|
| `ivk init` | create `.ivk/` skeleton |
| `ivk init --agent-instructions` | also generate `AGENTS.md` + `skills/ivk/*` |
| `ivk status [--json]` | one-shot summary across all workspaces |
| `ivk ws ls [--json]` | list workspaces |
| `ivk ws show <name|id> [--json]` | show one workspace |
| `ivk ws diff <name|id>` | git diff vs base snapshot |
| `ivk ws rm <name|id>` | delete a workspace |
| `ivk ch new <name|id>` | snapshot the workspace as a changeset |
| `ivk export <ch-id> [<branch>]` | export to a Git branch |
| `ivk ship <name|id>` | changeset + export + push + open PR |
| `ivk gc` | reclaim disk |

Until they exist, prefer `git` directly inside the workspace for status, diff, and commit work — the workspace IS a normal git worktree.
