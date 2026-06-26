# ivk safety rules

Read these once. They protect your work, the user's work, and the integrity of the source repo.

## Hard rules (never break these)

1. **Never edit files in the base repo root once a workspace exists.** Always work inside `.ivk/workspaces/<name>/`. Edits outside the workspace bypass ivk's lifecycle tracking and can be lost on `ivk gc`.
2. **Never create a manual `git worktree add`.** Use `ivk new`. Manual worktrees aren't registered and won't be cleaned up by `ivk gc`.
3. **Never delete `.ivk/`.** It contains the workspace registry and per-workspace admin entries. Use `ivk gc` to reclaim disk safely.
4. **Never run `git push` from inside a workspace** unless the user explicitly asked. Use `ivk export` / `ivk ship` so the export goes through ivk's auditable path.
5. **Never `--force` anything** without first running `ivk doctor --agent --json` and reading what it says.

## Soft rules (defaults you can override with reason)

6. **Prefer `ivk new` over `git checkout -b`.** Branches and workspaces solve different problems — workspaces give you a real disk-isolated place to run a build; branches don't.
7. **One workspace per task, not one workspace per branch.** Reuse a workspace only if you're iterating on the same task.
8. **Discard failed attempts.** `ivk ws rm <name>` (Phase 2) is cheaper than carrying broken state forward.
9. **Don't share workspaces between agents.** Each agent should get its own `ivk new <agent-id>-<task>`.

## Known footguns

- **Workspaces inherit the base repo's `.gitignore`.** Files ignored in the base are also ignored in the workspace. If you create new files that need tracking, `git add` them inside the workspace.
- **`pnpm install` inside a workspace shares the global pnpm store.** Multiple workspaces won't duplicate dependency disk. (See `results/build-summary.md`.)
- **`cargo build` inside a workspace re-uses the cloned `target/`.** If you want all workspaces to share one incremental build cache, set `CARGO_TARGET_DIR=$HOME/.cache/ivk/<repo-id>/cargo-target` before invoking cargo.
- **Editing a file via clonefile-shared blocks triggers copy-on-write per block.** First write to a file in a workspace breaks the share for that file's blocks — that's the safe, intended behavior.
- **`du -sh .ivk/workspaces/` lies on APFS.** `du` does not know about clonefile sharing. Always use `df` deltas for true disk consumption.

## When unsure

Run `ivk doctor --agent --json` and follow its `next_command`. If the response is ambiguous, fall back to:

```bash
ivk help --agent
```

which returns the full golden path and rule set as JSON.
