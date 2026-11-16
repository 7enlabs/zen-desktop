# Security Sweep W-7 - C1: MCP server auto-spawn (Critical)

**Finding C1 (R7 security sweep, 2026-10-17). Severity: Critical.**

## Finding

Project-scope MCP servers (`<cwd>/.zen/mcp.json`) were spawned at session
start **before any user approval**. Opening a cloned repository executed remote
code via a malicious `.zen/mcp.json` - local command execution the moment
the project was opened.

## Exploit sketch

1. Attacker publishes a repo containing `.zen/mcp.json` with a server
   whose `command` points to a payload (e.g. a stdio server that exfiltrates
   `~/.ssh` before responding).
2. Victim clones the repo and opens it in Zen Desktop.
3. Desktop spawns every configured MCP server at session start - no approval
   gate - and the payload runs with the user's privileges.

## Fix (from sweep branch `lead/security-sweep`, commit `bc7179f`)

`engine/src/mcp.rs` was changed so project-scope servers are never spawned
automatically:

- New `McpScope` enum: `TrustedConfig` (CLI scopes, `~/.zen/mcp.json`)
  vs `Project` (`<cwd>/.zen/mcp.json` - untrusted).
- `load_desktop_mcp_configs()` now returns `DesktopMcpConfigs =
  (spawnable, project_pending, provenance, skips)`. Project-scope servers go
  into `project_pending`, never into `spawnable`.
- `McpIntegration` holds project servers in `pending_project` and spawns
  them lazily via `approve_server()` after explicit user approval.
- Project shadowing of trusted-scope names is refused (C8 hardening rides
  with the same branch).

The fix carries a regression test on the sweep branch exercising that
project-scope servers land in `project_pending` and are never spawned at
startup (`pending_servers()` accessor).

## Landing plan

**This branch contains only this record - no code changes.** `mcp.rs` does not
exist on `main` (the MCP integration is an unmerged feature branch). The fix
rides with the MCP feature branch when it merges; the sweep branch
`lead/security-sweep` (commit `bc7179f`) holds the reference implementation.
This MR exists so the Critical finding is tracked as a standalone reviewable
unit, per roadmap FINAL §0A (W-7).
