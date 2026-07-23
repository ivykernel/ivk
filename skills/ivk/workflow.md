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

# Step 7. (Phase 3, coming) Snapshot + export.
ivk ch new fix-login
ivk export <ch-id> agent/fix-login
# or: ivk ship fix-login
```

## Failed attempt — discard and retry

```bash
ivk new attempt-1
cd .ivk/workspaces/attempt-1
# ... attempt fails (tests fail, can't figure it out, etc.)
cd ..  # back to repo root
ivk ws rm attempt-1                    # Phase 2, coming
ivk new attempt-2                       # try a fresh angle
```

## Multi-attempt (try N approaches in parallel)

```bash
# Spin up three candidate solutions.
ivk new attempt-{a,b,c}

# Hand each to a different agent / model, OR work them one after another.
# When done, compare:
ivk ws ls                               # Phase 2, coming
ivk ws diff attempt-a                   # see what each did
ivk ws diff attempt-b
ivk ws diff attempt-c

# Pick the winner, export it, discard the rest.
ivk export <best-ch-id> agent/fix-login # Phase 3
ivk ws rm attempt-b attempt-c           # Phase 2
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
ivk ws ls --json                        # Phase 2
ivk gc --failed                         # Phase 4: discard everything that didn't pass tests
```

## Changeset export (Phase 3, coming)

```bash
# Tests pass — record the changeset.
ivk ch new fix-login
# Output includes a changeset id like ch_01HABC.

# Does it merge cleanly onto the current integration point?
ivk ch check ch_01HABC                  # against HEAD; or: ivk ch check ch_01HABC main
# clean => export. Conflicts => rebase the workspace onto the target,
# resolve, then `ivk ch new fix-login` again and re-check.

# Export to a Git branch.
ivk export ch_01HABC agent/fix-login
# A branch named agent/fix-login now points at the changeset's tree.

# Push + PR (one of two paths):
ivk ship fix-login                      # all-in-one
# OR manually:
git push origin agent/fix-login
gh pr create
```

## Cleanup (Phase 4, coming)

```bash
ivk gc                                  # collect stale workspaces + orphan overlays
ivk ws rm --failed                      # remove every workspace whose last test pass failed
ivk ws rm --all-discarded               # nuclear: remove everything not exported
```

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
