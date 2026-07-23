# ivk CLI reference (agent-facing)

Every command below supports `--json` for machine output and `--agent` for the agent-friendly form (adds `recommended_next_steps`).

## `ivk --version`

Prints the binary version (e.g. `ivk 0.0.1`). No JSON.

## `ivk help [--agent]`

Prints the golden path. With `--agent` returns the workflow as JSON with `next_command` pointing at `ivk doctor --agent --json`.

## `ivk doctor [--agent] [--json] [--repair]`

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

At a repo root with `.ivk/`, the response also carries a `registry` block
describing the SQLite state registry (`.ivk/db.sqlite`):

```json
{
  "registry": {
    "db_present": true,
    "tracked_workspaces": 3,
    "in_flight": [ { "name": "ghost", "state": "creating" } ],
    "stale_rows": [ "vanished" ]
  }
}
```

`in_flight` rows are interrupted operations (a killed `ivk new` / `ivk ws
rm`); `stale_rows` are registry entries whose directory is gone. When either
is non-empty, `next_command` becomes `ivk doctor --repair`. Running with
`--repair` rolls back half-created workspaces, completes half-finished
removals, drops stale rows, and reports what it did in a `repair` block
(`rolled_back` / `completed_removals` / `dropped_stale_rows`).

## `ivk new <name> [<name>...] [--from <rev>] [--json] [--agent]`

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

`--from <rev>` bases the workspace on a revision other than HEAD (branch,
tag, sha, `HEAD~2`, ...). The tree is CoW-cloned as usual, then only paths
differing between HEAD and `<rev>` are rewritten; git-ignored files (caches,
build artifacts) survive, so dependency sharing holds for old bases too. A
revision that does not resolve fails fast with `error.code =
"invalid_revision"` before any workspace is touched.

Common errors:

| code | meaning | recovery |
|---|---|---|
| `not_a_git_repo`   | cwd has no `.git/` | `git init` |
| `missing_argument` | no name given      | pass at least one name |

## `ivk ws new <name>...` (same as `ivk new`)

Fully qualified form. Use this in scripts where ambiguity matters.

## `ivk ws du [<name>...] [--json] [--agent]`

Storage estimation per workspace (alias: `ivk du`). Reports `apparent`
(byte sum) and `allocated` (filesystem blocks) per workspace plus totals,
sorted largest-first. CoW caveat: shared blocks count once per workspace
here, so real disk growth is lower until files diverge — `df` is ground
truth. `--agent` names the largest workspace and suggests `ivk ws rm` +
`ivk gc`.

## Exit codes

- `0` — success
- `1` — runtime error (file system, git invocation, etc.)
- `2` — usage error (bad arguments)

## Currently implemented

| command | summary |
|---|---|
| `ivk init` | create `.ivk/` skeleton |
| `ivk init --agent-instructions` | also generate `AGENTS.md` + `skills/ivk/*` |
| `ivk status [--json]` | one-shot summary across all workspaces |
| `ivk ws ls [--json]` | list workspaces |
| `ivk ws show <name> [--json]` | show one workspace |
| `ivk ws diff <name>` | git diff vs base snapshot |
| `ivk ws rm <name>` | delete a workspace |
| `ivk ch new <name>` | snapshot the workspace as a changeset (auto-commits inside the worktree) |
| `ivk ch ls [--json]` | list changesets |
| `ivk ch show <ch-id> [--json]` | show one changeset |
| `ivk ch check <ch-id> [<rev>] [--json]` | conflict status: does the changeset merge cleanly onto `<rev>` (default `HEAD`)? In-memory merge — no working tree touched; exit 0 for both verdicts, read `clean` / `conflict_paths` |
| `ivk export <ch-id> [<branch>]` | point a git branch (default `agent/<ws>`) at the changeset commit |
| `ivk patch <ch-id> [<path>]` | write a unified-diff `.patch` file (default `./patches/<ch-id>.patch`) |
| `ivk gc [--dry-run]` | prune orphan workspaces / git worktree admin; report `bytes_reclaimed` and `orphaned_changeset_refs` |
| `ivk ws rm --all      [--yes] [--force] [--dry-run]` | bulk remove every workspace; dirty ones are skipped unless `--force` |
| `ivk ws rm --exported [--yes] [--force] [--dry-run]` | remove every workspace whose HEAD equals its `refs/heads/agent/<ws>` branch |
| `ivk bench spawn [--count N]` | materialize N workspaces from HEAD; report timings + disk delta |
| `ivk bench compare-git-worktree [--count N]` | run both arms in randomized order; emit `comparison.lp_blurb` |
| `ivk bench disk [--count N]` | apparent / blocks / df-delta triad + `ratios.lp_blurb` |
| `ivk bench gc [--count N]` | synthetic gc throughput; reports `bytes_reclaimed` + `ms_per_workspace` |
| `ivk new --from <rev>` | base workspaces on a non-HEAD revision (ignored files kept) |
| `ivk ws du [<name>...]` | apparent + allocated bytes per workspace (alias `ivk du`) |
| `ivk doctor --repair` | roll back / complete interrupted operations; recover unrecorded changesets |

## Planned, not yet implemented

| command | summary |
|---|---|
| `ivk ship <name>` | changeset + export + push + open PR |
| `ivk ws rm --failed` | needs test-result tracking — refuses with `unsupported_flag` today |
| `ivk ws rm --all-discarded` | needs an exported/discarded marker — refuses with `unsupported_flag` today, use `--exported` or `--all` |
| `ivk bench --from <rev>` | benches always run against HEAD; `ivk new --from <rev>` covers non-HEAD workspaces |
| `ivk bench matrix` | wraps `scripts/bench/collect.sh` — dev-only, deferred to Phase 5+ |

Until `ivk ship` lands, do the push/PR step manually: `git push origin <branch> && gh pr create`.
