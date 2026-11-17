# Upstream proposals for Zen CLI (atzenn/zencode)

## Research

The Desktop engine consumes the CLI's crates (`zen-agent`, `zen-tools`, `zen-core`) as pinned git dependencies and, in headless mode, the `zen -p ... --output-format stream-json` contract. While building the engine (PRs #1–#9) and reviewing the stream-json contract, ZD Leader found several issues that can only be fixed in the CLI repo. This file collects them; each is actionable by the CLI team. Desktop-side workarounds are noted where they exist.

## Ideas (proposals)

### U1. Version field in stream-json output

- **Problem:** Event-log session files carry `"v":1` on every record, but `stream-json` stdout lines have no version field. A Desktop client cannot detect a breaking schema change except by failing to parse.
- **Proposal:** Add `"v":1` to every stream-json line (same convention as the event log). Bump on breaking changes and note it in the CLI CHANGELOG.
- **Status:** Open. (CLI PR #4 made `AgentEvent` `#[non_exhaustive]` but did not add the version field.)
- **Desktop workaround:** `engine/src/events.rs` validates the Desktop-side event schema (v1) with insta contract tests, but it cannot detect CLI-side drift.

### U2. ToolSchemaMode in AgentConfig (progressive disclosure)

- **Problem:** The engine can measure and enforce a prompt budget (g4: ≤1,200 tokens, `engine/src/prompt_budget.rs`), but truly lazy tool schemas - names + one-liners first, full JSON schemas on demand - need cooperation inside the `zen-agent` turn loop, which the engine cannot inject.
- **Proposal:** Add `ToolSchemaMode { Full, Minimal }` to `AgentConfig`. In `Minimal` mode the agent sends compact tool definitions and fetches full schemas through a lightweight internal mechanism before each tool's first use.
- **Status:** Proposed (design only; no CLI change drafted).
- **Desktop workaround:** Budget measurement + `ensure_within_budget()` fail-fast guard in `run_task_with_sink`; currently ~782/1200 tokens with the 4 default tools.

### U3. Align on PreToolUse hook usage

- **Problem:** `zen-agent` already fires `HookEvent::PreToolUse` before every tool call, but the Desktop engine hardcodes `NoHooks`, so the event is dead weight on our side.
- **Proposal:** None required from the CLI - this is a note. If the CLI team adds hook events or changes `HookRunner` semantics, please announce it; it affects Desktop's hook chain.
- **Status:** Noted. Desktop now has its own type-safe `PreToolUseHook` (`engine/src/hooks.rs`, PR #8: Allow/Deny/Modify + ordered chain) wired through `RunOptions::pre_tool_use_hooks`.
- **Desktop workaround:** N/A - implemented engine-side.

### U4. RateGate / token-budget placement

- **Problem:** Unclear whether per-provider daily quota + model fallback should live in the CLI or in Desktop.
- **Proposal:** See atzenn/zencode#10 ("[desktop] RateGate + token budget: should they live in CLI or Desktop?").
- **Status:** Open, awaiting CLI-team decision. No duplicate filed here.
- **Desktop workaround:** P2 hook infrastructure (PR #8) is ready to host the policy once its placement is decided.

### U5. Heartbeat during long tool runs

- **Problem:** `stream-json` emits nothing while a tool call (e.g. a long Bash command) runs. A Desktop UI cannot distinguish "working" from "hung" and cannot show progress.
- **Proposal:** Emit a lightweight heartbeat line (e.g. `{"type":"heartbeat","v":1,"tool_use_id":"..."}`) every N seconds while a tool call is in flight.
- **Status:** Open.
- **Desktop workaround:** None - the UI currently shows a spinner with no progress signal.

### U6. Structured spill-file reference

- **Problem:** When tool output exceeds the char limit, the CLI embeds the spill-file path in prose ("Full output saved to \<scratch\>/tool-output/\<id\>.txt"). Desktop must regex prose to offer a "view full output" button.
- **Proposal:** Add a structured field to the tool-result line, e.g. `"spill_file": "<path>"` (null when absent), alongside the prose.
- **Status:** Open.
- **Desktop workaround:** Regex extraction (fragile).

## Results - what Desktop does meanwhile

- All Desktop-side mitigations above are implemented on `lead/*` branches and proposed as PRs; none touch CLI code.
- When the CLI team accepts a proposal, Desktop adopts it and drops the workaround; until then the workarounds stay.
- New findings go here first; once filed upstream, they should become issues in atzenn/zencode with the `desktop` label.

## Desktop branches parked for the CLI team (2026-10-18 review)

These `lead/*` branches on `gitlab.com/7enlabs/zen-desktop` implement agent-core behaviour.
They are **not** merged into Desktop. Each line is a pointer for the CLI team (`gitlab.com/atzenn/zencode`):
read the branch, port the idea into the CLI, then Desktop consumes it through a rev bump.
Full verdict table: `docs/reviews/2026-10-18-lead-branches.md`.

| Branch | Proposal for the CLI |
|---|---|
| `lead/agent-skills` | Skill discovery/loading in the agent loop (SKILL.md registry). |
| `lead/skills-index` | Skill index/search on top of agent-skills (depends on it). |
| `lead/approval-flow` | Interactive tool-approval flow in the permission checker (pairs with U3). |
| `lead/compaction` | Context compaction strategy in the agent loop. |
| `lead/g3-rategate` | RateGate / token-budget gate (see U4). |
| `lead/g4-prompt-budget` | Prompt budgeting (`prompt_budget.rs`); drop the bundled spec-kit scaffold. |
| `lead/integration-test` | Agent-loop integration tests; belong next to the loop. |
| `lead/linux-sandbox` | OS sandbox for Bash on Linux (zen-tools). |
| `lead/macos-sandbox` | OS sandbox for Bash on macOS (zen-tools). |
| `lead/windows-sandbox` | OS sandbox for Bash on Windows (zen-tools). |
| `lead/wasmtime-sandbox` | WASM sandbox for tools (zen-tools). |
| `lead/atomic-writes` | Atomic file writes for Write/Edit tools (zen-tools), incl. Windows readonly fix. |
| `lead/memory-polish` | Agent memory handling. |
| `lead/p2-pretooluse-hook` | PreToolUse hook (see U3). |
| `lead/p3-read-tracking` | Read-before-edit tracking in the tool layer. |
| `lead/parallel-tools` | Parallel tool execution in the agent loop. |
| `lead/plan-mode` | Plan mode in the agent loop. |
| `lead/repo-map` | Repository map for context building. |
| `lead/smart-router` | In-loop model routing (Desktop keeps the pure `smart_router.rs` from R10 for model choice). |
| `lead/user-learning` | Learning user preferences across sessions. |
| `lead/verify-loop`, `lead-verify-loop` | Verify loop `--until` (re-run tests until green). |

- **CLI: declare a license** (Z App, 2026-10-18): `zen-core`, `zen-agent`, `zen-tools` have no `license` field and the repo has no LICENSE file, so `cargo deny check licenses` flags them as unlicensed. Desktop marks them `LicenseRef-Zen-FirstParty` in `deny.toml` until the CLI adds `license = "..."` to its workspace `Cargo.toml`.
