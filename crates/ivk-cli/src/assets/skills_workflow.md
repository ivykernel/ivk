# ivk workflows

## Single task (the default)

```bash
# Step 1. Discover state.
ivk doctor --agent --json
# Expected: not yet in a workspace.

# Step 2. Create a workspace for the task.
ivk new fix-login

# Step 3. Move into it.
cd .ivk/workspaces/fix-login

# Step 4. Verify.
ivk doctor --agent --json
# Expected: inside_ivk_workspace=true, workspace_status=clean.

# Step 5. Make changes, run tests.
# ... edit files, run `npm test` / `cargo test` / etc.

# Step 6. Verify again.
ivk doctor --agent --json
# Expected: workspace_status=dirty, has_changes=true.

# Step 7. Snapshot + export.
ivk ch new fix-login
ivk export <ch-id> agent/fix-login
ivk patch  <ch-id>                       # (optional) emit a .patch file
# Coming: ivk ship fix-login (all-in-one with push + gh pr create).
```

## Failed attempt — discard and retry

```bash
ivk new attempt-1
cd .ivk/workspaces/attempt-1
# ... attempt fails (tests fail, can't figure it out, etc.)
cd ..  # back to repo root
ivk ws rm attempt-1
ivk new attempt-2                       # try a fresh angle
```

## Multi-attempt (try N approaches in parallel)

```bash
# Spin up three candidate solutions.
ivk new attempt-{a,b,c}

# Hand each to a different agent / model, OR work them one after another.
# When done, compare:
ivk ws ls
ivk ws diff attempt-a                   # see what each did
ivk ws diff attempt-b
ivk ws diff attempt-c

# Pick the winner, export it, discard the rest.
ivk ch new attempt-a
ivk export <best-ch-id> agent/fix-login
ivk ws rm attempt-b attempt-c
```

## Multi-agent (many agents on the same repo)

```bash
# Orchestrator spawns one workspace per agent.
ivk new agent-1 agent-2 agent-3 ... agent-N

# Each agent works in its own workspace:
#   Agent 1: cd .ivk/workspaces/agent-1 && do its work
#   Agent 2: cd .ivk/workspaces/agent-2 && do its work
#   ...

# Collect successful results.
ivk ws ls --json
ivk ws rm --exported --yes              # discard workspaces preserved as agent/<ws> branches
ivk gc                                  # prune orphan admin entries; report bytes reclaimed
```

## Changeset export

```bash
# Tests pass — record the changeset.
ivk ch new fix-login
# Output includes a changeset id like ch_<sha12>.

# Does it merge cleanly onto the current integration point?
ivk ch check ch_<sha12>                  # against HEAD; or: ivk ch check ch_<sha12> main
# clean => export. Conflicts => rebase the workspace onto the target,
# resolve, then `ivk ch new fix-login` again and re-check.

# Export to a Git branch.
ivk export ch_<sha12> agent/fix-login
# A branch named agent/fix-login now points at the changeset's commit.

# (Optional) write a unified-diff .patch file:
ivk patch ch_<sha12>                     # default: ./patches/ch_<sha12>.patch

# Push + PR — manual today (ivk ship coming):
git push origin agent/fix-login
gh pr create
```

## Cleanup

```bash
ivk gc                                  # prune orphan workspaces / git worktree admin entries; report bytes reclaimed
ivk gc --dry-run                        # preview what gc would remove without touching disk
ivk ws rm --exported --yes              # remove every workspace whose HEAD matches a refs/heads/agent/<ws> branch
ivk ws rm --exported --yes --force      # ...including dirty ones (uncommitted edits are discarded — sure?)
ivk ws rm --all --yes                   # nuclear: remove every workspace (dirty ones skipped unless --force)
ivk ws rm --all --yes --dry-run         # preview before nuking

# Deferred to a future release (will fail today with error code `unsupported_flag`):
# ivk ws rm --failed         # needs test-result tracking, not in v0.0.1
# ivk ws rm --all-discarded  # needs an exported/discarded marker, not in v0.0.1
```

`ivk gc` and bulk `ivk ws rm` share a `.ivk/.gc.lock`; only one destructive pass can run at a time.
gc NEVER deletes `.ivk/changesets/*.json`; if removing a workspace would orphan a changeset, the JSON output lists those in `orphaned_changeset_refs`.

## Recovery patterns

If you lose track of where you are, ALWAYS:

```bash
ivk doctor --agent --json
```

and follow the `next_command` it returns.

If `ivk doctor` says `inside_ivk_workspace: false` but you thought you were inside one, you're probably at the repo root. `cd .ivk/workspaces/<name>` to enter.

If a `clonewt` or workspace materialization fails partway, run:

```bash
git -C $(ivk doctor --json | jq -r .repo_root) worktree prune
rm -rf .ivk/workspaces/<broken-name>
ivk new <broken-name>
```

(Phase 4's `ivk gc` will automate this.)
