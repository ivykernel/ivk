# Ivy Kernel (`ivk`) Design Philosophy

## One-line concept

**Ivy Kernel (`ivk`) is a parallel workspace kernel for AI agents.**

It is designed for a world where many AI agents work on the same codebase in parallel, each with its own lightweight, isolated workspace.

> If Jujutsu is change-native, Ivy Kernel is workspace-native.

Or, more directly:

> Jujutsu makes one developer's local history easier.  
> Ivy Kernel makes many agents' parallel workspaces cheap and manageable.

---

## Why Ivy Kernel exists

Git is excellent at storing history.

It gives us:

- content-addressed objects
- commits
- trees
- branches
- merges
- distributed synchronization

But Git was designed around a human-centered workflow:

```text
working tree
  ↓ git add
index
  ↓ git commit
commit
  ↓ branch / merge / push
```

This model works well when a human developer is working on one task at a time.

However, AI-native development changes the shape of the problem.

In an AI-agent workflow, we may have:

```text
Agent A: fix login bug
Agent B: refactor auth module
Agent C: update tests
Agent D: migrate config
...
Agent N: explore an alternative implementation
```

Each agent needs an isolated workspace.

With Git today, this usually means creating many worktrees:

```text
repo/
worktrees/
  agent-a/
  agent-b/
  agent-c/
  agent-d/
  ...
```

This works, but it becomes messy and heavy.

The real problem is not Git's commit graph.  
The real problem is that Git does not treat parallel workspaces as a first-class abstraction.

---

## The core problem

AI-agent development needs cheap, manageable, disposable workspaces.

The kernel should make it easy to:

- create many agent workspaces from the same base snapshot
- isolate each workspace
- track workspace lifecycle
- represent each workspace as `base snapshot + overlay`
- materialize a workspace only when needed
- discard failed or abandoned workspaces quickly
- export useful changes back to Git
- manage cache efficiently across workspaces

In short:

```text
N agents
N lightweight workspaces
1 shared base snapshot
N overlays
shared dependency/cache layer where possible
```

The goal is to avoid this:

```text
N agents
N full worktrees
N dependency installs
N build caches
N stale directories
```

---

## Ivy Kernel is workspace-native

Git is commit/branch/working-tree-native.

Jujutsu is change/revision-graph-native.

Ivy Kernel is workspace/overlay/changeset-native.

```text
Git:
  commit / branch / working tree are central
  humans stage and commit changes
  GitHub provides review and collaboration

Jujutsu:
  change / revision graph are central
  working copy is also a revision
  local history editing becomes easier
  Git compatibility is preserved

Ivy Kernel:
  workspace / overlay / changeset are central
  many agents can work in parallel
  workspaces are lightweight and disposable
  changes can be exported back to Git
```

Ivy Kernel does not try to replace Git immediately.

Instead:

```text
Internally:
  workspace / overlay / changeset

Externally:
  git commit / branch / patch / GitHub PR
```

---

## What Ivy Kernel should learn from Jujutsu

Jujutsu is not the same thing as Ivy Kernel, but it is an important design reference.

Jujutsu is a change-native VCS.  
Ivy Kernel is intended to be a parallel workspace-native kernel.

Still, Ivy Kernel should learn at least three major lessons from Jujutsu.

---

### 1. Do not keep the working copy outside the graph

In Git, uncommitted changes live outside the commit graph.

```text
A --- B --- C

working tree changes: outside the graph
```

This creates an awkward state:

```text
uncommitted changes
stash
index
partial staging
dirty working tree
```

Jujutsu improves this by treating the working copy itself as a revision.

```text
A --- B --- C --- @

@ = working-copy revision
```

This idea is highly relevant to AI agents.

An AI agent's intermediate work should not be an unknown, dirty directory.

It should be represented as a graph-visible temporary workspace or change.

```text
base snapshot
  ├─ workspace A
  ├─ workspace B
  └─ workspace C
```

Even if the work is temporary, failed, or experimental, it should still be observable and manageable.

---

### 2. Make change units more important than branch names

In Git, users often start by creating a branch.

```bash
git switch -c feature/foo
```

But in AI-agent workflows, branch names are not the core abstraction.

The real question is:

> What change did this agent produce?

For AI agents, the important objects are:

- workspace
- change
- changeset
- proposal

A branch is only one possible export format.

Ivy Kernel should therefore avoid making branch names central to the internal model.

Instead, the internal flow should look more like:

```text
base snapshot
  ↓
workspace
  ↓
overlay
  ↓
changeset
  ↓
export to Git branch / patch / PR
```

---

### 3. Do not abandon Git compatibility

Jujutsu is smart because it does not reject Git.

It improves the local development model while preserving Git compatibility.

Ivy Kernel should follow the same strategy.

The internal model can be new:

```text
workspace
overlay
changeset
workspace lifecycle
materialization
cache
```

But the external interface should still support:

```text
git commit
git branch
git patch
GitHub PR
Git remote
```

This makes adoption much easier.

Ivy Kernel should be able to say:

> Use Ivy Kernel internally for cheap parallel workspaces.  
> Export the result back to Git when you want to review, merge, or share it.

---

## Core concepts

Ivy Kernel should stay small.

The true kernel should focus only on the minimum set of concepts needed to support parallel AI-agent workspaces.

---

### Snapshot

A snapshot represents a content-addressed state of the codebase.

In the first implementation, this can simply map to a Git commit.

```text
snapshot_id = git commit hash
```

Later, Ivy Kernel may have its own object store, but that is not required for the MVP.

---

### Workspace

A workspace is a lightweight isolated working context created from a base snapshot.

```text
workspace:
  id
  base_snapshot
  overlay
  status
  created_at
```

A workspace is not necessarily a full physical directory.

It is a logical working context.

---

### Overlay

An overlay stores the difference between the base snapshot and the workspace.

```text
overlay:
  added files
  modified files
  deleted files
```

This is the core idea.

Instead of creating many full worktrees, Ivy Kernel should represent each agent workspace as:

```text
base snapshot + overlay
```

---

### Materialized workspace

Most existing tools expect a normal filesystem.

For example:

- npm
- pnpm
- cargo
- go
- gradle
- flutter
- pytest
- eslint
- TypeScript language server
- Claude Code
- Codex
- Cursor agents

So Ivy Kernel must be able to materialize a workspace into a normal directory.

```bash
ivk ws mount <workspace-id> ./workspaces/foo
```

The first version can use physical directories.

Later versions can use:

- copy-on-write
- filesystem overlays
- FUSE
- container snapshotters
- shared caches

---

### ChangeSet

A changeset is a finalized unit of change produced from a workspace.

```text
changeset:
  id
  base_snapshot
  result_snapshot
  touched_paths
  file_diff
```

A changeset should be exportable to Git.

```bash
ivk ch create <workspace-id>
ivk git export <changeset-id> --branch agent/foo
```

The kernel's changeset should remain minimal.

It should represent what changed, not the full review discussion.

---

### Workspace lifecycle

Ivy Kernel should manage workspace lifecycle directly.

A workspace can be:

```text
created
materialized
active
changed
converted to changeset
exported
discarded
garbage-collected
```

This is one of the key differences from plain Git worktrees.

Git worktrees exist as directories.  
Ivy workspaces should exist as lifecycle-managed kernel objects.

---

### Cache awareness

Parallel AI-agent workspaces become expensive when each workspace duplicates dependencies and build artifacts.

Ivy Kernel should eventually manage or integrate with shared caches.

Examples:

```text
node_modules
pnpm store
Cargo target cache
Go build cache
Gradle cache
Flutter build artifacts
Python virtualenv / uv cache
```

This does not need to be solved perfectly in the MVP.

But it should be part of the design direction.

---

## What belongs in Ivy Kernel

Ivy Kernel should remain small.

The kernel should handle:

```text
snapshot
workspace
overlay
materialization
changeset
git export
workspace lifecycle
garbage collection
basic cache strategy
```

This is the real core.

---

## What does not belong in Ivy Kernel

Ivy Kernel should not try to become GitHub, GitLab, Linear, Devin, or a full CI/CD platform.

The following should not be in the kernel:

```text
review UI
comments
approval workflow
organization management
issue tracking
CI/CD dashboard
agent orchestration UI
semantic risk analysis UI
hosted collaboration
permissions dashboard
team activity feed
```

Those belong in a higher-level product.

---

## IvyHub

IvyHub is the review and collaboration layer built on top of Ivy Kernel.

The relationship should be similar to Git and GitHub:

```text
Git      -> GitHub
Ivy Kernel -> IvyHub
```

Or:

```text
ivk:
  local kernel / CLI / workspace engine

IvyHub:
  hosted review / collaboration / visibility layer
```

IvyHub can handle:

```text
changeset review
comments
approval
CI/test evidence
agent run logs
risk summaries
workspace history visualization
team collaboration
GitHub integration
```

The boundary should be:

```text
Ivy Kernel creates facts.
IvyHub helps humans and agents judge those facts.
```

For example, Ivy Kernel should produce:

```text
changeset id
base snapshot
result snapshot
file diff
touched paths
conflict status
exportable patch
```

IvyHub can add:

```text
review comments
AI summary
risk analysis
test result display
approval status
merge decision
discussion
```

---

## MVP scope

The first version of Ivy Kernel should be intentionally small.

A reasonable MVP CLI could be:

```bash
ivk init

ivk ws new --from main
ivk ws ls
ivk ws mount <workspace-id> ./workspaces/foo
ivk ws diff <workspace-id>
ivk ws rm <workspace-id>

ivk ch new <workspace-id>
ivk ch show <changeset-id>

ivk git export <changeset-id> --branch agent/foo

ivk gc
```

The MVP does not need to solve every hard problem.

It only needs to prove that:

> Many AI agents can work from the same base codebase using lightweight, lifecycle-managed workspaces, then export useful changes back to Git.

---

## Non-goals for MVP

The MVP should not include:

```text
full Git replacement
hosted service
review UI
CI system
semantic merge
complex permission model
custom object database
distributed sync protocol
AI agent marketplace
IDE extension
```

Those can come later.

The first goal is to build a credible workspace kernel.

---

## Positioning

### Against Git

Git is the history kernel.

Ivy Kernel is the workspace kernel.

```text
Git stores what happened.
Ivy Kernel manages where parallel work happens.
```

Ivy Kernel should not fight Git at first.  
It should use Git as the external compatibility layer.

---

### Against Jujutsu

Jujutsu is change-native.

Ivy Kernel is workspace-native.

```text
Jujutsu makes one developer's local history easier.
Ivy Kernel makes many agents' parallel workspaces cheap and manageable.
```

Jujutsu improves how developers manipulate local change history.

Ivy Kernel improves how many AI agents create, isolate, discard, and export parallel workspaces.

---

### Against GitHub

GitHub is a collaboration layer for Git.

IvyHub can become a collaboration layer for Ivy Kernel.

But Ivy Kernel itself should not become IvyHub.

The kernel should stay small, composable, and local-first.

---

## Long-term direction

If the MVP works, Ivy Kernel can evolve in stages.

### Phase 1: Git-compatible workspace manager

Use Git internally where possible.

Manage workspaces, overlays, changesets, and export.

### Phase 2: Smarter materialization

Reduce physical worktree cost using copy-on-write or overlay-based materialization.

### Phase 3: Cache-aware workspaces

Share dependency and build caches across agent workspaces.

### Phase 4: Changeset registry

Track changesets, workspace lifecycle, and agent-produced changes in a durable local registry.

### Phase 5: IvyHub

Add hosted review, CI evidence, comments, approvals, and team collaboration.

### Phase 6: Optional custom object store

Only if Git becomes the bottleneck, introduce a deeper internal object model.

Until then, Git compatibility should remain a core adoption strategy.

---

## Design mantra

```text
Keep the kernel small.
Make workspaces first-class.
Treat agent work as graph-visible.
Use overlays instead of heavy worktrees.
Export to Git, do not fight Git.
Leave review and collaboration to IvyHub.
```

---

## Short pitch

**Ivy Kernel (`ivk`) is a parallel workspace kernel for AI agents.**

It lets many agents work from the same base codebase using lightweight isolated workspaces, tracks their changes as changesets, and exports useful results back to Git.

Git stores history.  
Jujutsu makes local changes easier.  
Ivy Kernel makes parallel AI workspaces manageable.
