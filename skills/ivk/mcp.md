# ivk MCP interface (planned)

Status: **not implemented yet**. This document describes the intended MCP surface so an agent that does support MCP can know what to expect.

For Phase 0-3, drive `ivk` via the CLI and JSON output (see [`cli.md`](./cli.md)).

## Planned MCP tools

```text
ivk_doctor                  -> { ok, inside_ivk_workspace, workspace_name, ... }
ivk_create_workspace        ({ name, from?, with_git? }) -> { workspace_id, path, ... }
ivk_list_workspaces         -> [{ name, status, ... }]
ivk_get_workspace_status    ({ name }) -> { ... }
ivk_remove_workspace        ({ name }) -> { removed: true }
ivk_create_changeset        ({ name }) -> { changeset_id, ... }
ivk_export_changeset_to_git ({ changeset_id, branch? }) -> { branch, sha }
ivk_run_gc                  -> { reclaimed_bytes }
```

## Why MCP

The CLI + JSON form is sufficient for most agents (Claude Code, Codex, Cursor all support bash + tool-use). MCP becomes useful when:

- The agent runs in a sandbox without bash.
- The host (Claude Desktop, ChatGPT Desktop, etc.) wants typed, schema-validated calls.
- We want to enforce a stricter contract (e.g. always require `name` to be a single shell-safe identifier).

## Launching the (future) server

```bash
ivk mcp serve              # not implemented
```

Until then: shell out to `ivk` and parse the `--json` output.

## Schema sketch

The tools mirror the CLI subcommands. Errors come back as:

```json
{ "ok": false, "error": { "code": "<error_code>", "message": "<human-readable>" } }
```

with `code` from the catalog in `cli.md`.
