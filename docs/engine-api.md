# Engine API guide (`zen_engine`)

> Verified against `origin/main` on GitLab. Field and function names are taken
> from the actual source.

## Overview

The `engine/` crate is a thin assembly layer: it wires the Zen CLI's agent loop
(`zen-agent`), file/shell tools (`zen-tools`), and core types (`zen-core`) - all pulled
as git dependencies pinned to one CLI rev - against any OpenAI-compatible endpoint.
No agent core is rewritten here; the crate only assembles the pieces.

Crate layout:

```text
engine/
  src/lib.rs          RunOptions, default_tools(), run_task(), run_task_with_sink()
  src/events.rs       Versioned DesktopEvent schema + RecordingSink (the UI contract)
  src/prompt_budget.rs  g4 token budget: chars/4 estimate + fail-fast guard
  src/hooks.rs          PreToolUseHook trait + ordered HookChain
  src/read_tracking.rs  P3 read-before-write defense-in-depth
  src/bin/zen-desktop.rs    `zen-desktop run` / `zen-desktop eval` CLI
  tests/fake_llm.rs            Integration tests against a scripted localhost LLM
  tests/snapshots/             insta contract snapshot for the event stream
```

Git dependencies (`engine/Cargo.toml`): `zen-core`, `zen-agent`, `zen-tools` from
`https://gitlab.com/atzenn/zencode` at a pinned rev.

## Quick start

```rust
use std::sync::Arc;
use zen_engine::{events::RecordingSink, run_task_with_sink, RunOptions, scope::ScopeDecl};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut opts = RunOptions::new(
        "https://api.experientiallabs.ai/api/v1", // OpenAI-compatible base URL
        "deepseek-v4.1-flash",
        "/tmp/workdir",                      // cwd the tools operate in
    );
    // R3 scope guard: declare the dirs the agent may touch (deny-by-default).
    opts.scope = Some(ScopeDecl {
        files: vec![],
        dirs: vec!["/tmp/workdir".into()],
        description: "working directory".into(),
    });
    let sink = Arc::new(RecordingSink::new());
    let result = run_task_with_sink(&opts, "Create hello.txt with: hi", sink.clone()).await?;
    println!("success: {}", result.success);
    println!("{}", sink.to_ndjson()?); // versioned event log, one JSON object per line
    Ok(())
}
```

Notes: `run_task` needs no key for keyless free tiers (`api_key: None`); pass
`opts.api_key = Some(...)` for providers that need a bearer token. Tool approval is
**deny-by-default** - declare a scope via `opts.scope` (R3); the legacy
`dangerous_allow_all` flag is deprecated.

## Core types

**`RunOptions`** (all fields `pub`):

| Field | Type | Default (`RunOptions::new`) |
|---|---|---|
| `base_url` | `String` | required - non-empty, must start with `http://`/`https://` |
| `api_key` | `Option<String>` | `None` |
| `model` | `String` | required - non-empty |
| `fallback_models` | `Vec<String>` | `vec![]` |
| `cwd` | `PathBuf` | required - must be an existing directory |
| `max_turns` | `usize` | `12` |
| `system_prompt` | `String` | `"You are Zen CLI, a concise coding agent. …"` |
| `dangerous_allow_all` (deprecated) | `bool` | `false` - deny all tool calls unless `true` |
| `scope` (R3) | `Option<ScopeDecl>` | `None` - declare dirs the agent may touch |
| `pre_tool_use_hooks` | `Vec<Arc<dyn PreToolUseHook>>` | `vec![]` |

**`run_task(opts, task) -> anyhow::Result<RunResult>`** - run one task to completion.
Events go to a `NullSink` (discarded). Rejects empty tasks and non-directory `cwd`.

**`run_task_with_sink(opts, task, sink: Arc<RecordingSink>)`** - same, but
streams every agent event into the sink and appends a `DesktopEvent::SessionEnd` with
cost accounting when the turn finishes.

**`default_tools() -> ToolRegistry`** - the CLI's `Read`, `Edit`, `Write`, `Bash` tools.

**`default_tracked_tools() -> (ToolRegistry, ReadTracker)`** - same tools
routed through the read-tracking layer; prefer this for agent runs.

**`events::RecordingSink`** - collects events as versioned envelopes:
`events()` returns `Vec<Envelope>`; `to_ndjson()` returns `anyhow::Result<String>`
(no panics on serialization failure).

**`events::DesktopEvent`** - the UI contract, a stable projection of the CLI's
`AgentEvent` (converted via `From<AgentEvent>`). Variants: `AssistantMessage`,
`ToolResult`, `TextDelta`, `ApiRetry`, `CompactBoundary`, `PermissionDenied`,
`SystemMessage`, `Unknown { variant }`, and `SessionEnd` (`success`, `result`,
`num_turns`, `total_cost_usd`, `input_tokens`, `output_tokens`, `model`,
`duration_ms`). Each log line is `{"v":1,"seq":N,"type":"…",…}`.

**Versioning:** `events::SCHEMA_VERSION` is `1`; bump it on any breaking change and
re-review the insta snapshots. `parse_ndjson()` rejects lines with a missing (`v: 0`)
or newer version instead of misreading them - a CLI rev bump can never silently
change what the UI sees; the contract tests fail first.

**Additive variants are allowed in v1.** Adding a new variant to
`DesktopEvent` (e.g. `SessionEnd`) does **not** bump `SCHEMA_VERSION`; only
breaking changes to existing variants or fields do. Consumers must tolerate
unknown variants (see the `Unknown { variant }` catch-all) rather than fail
on them, because older readers will keep meeting newer event streams.
`SCHEMA_VERSION` moves only when the envelope shape or an existing variant's
meaning changes incompatibly.

## New modules

**`prompt_budget` (g4).** Keeps the static prompt (system prompt + tool
definitions) under a token budget, using the chars/4 estimate.

```rust
use zen_engine::prompt_budget::{ensure_within_budget, PROMPT_BUDGET_TOKENS};

// ~1200 token budget (chars/4 heuristic); fail fast before the run starts.
let used = ensure_within_budget(&opts.system_prompt, &tools)?; // Err(OverBudget) if > budget
```

Key items: `estimate_tokens(&str)`, `tool_definition_tokens(&ToolDefinition)`,
`static_prompt_tokens(&str, &ToolRegistry)`, `ensure_within_budget(...) ->
Result<usize, OverBudget>`, `OverBudget { used, budget }`. `run_task_with_sink`
enforces this automatically - a bloated prompt fails before the first turn.

**`hooks` (P2).** Engine-native pre-tool-use interception: each tool is
wrapped so every call first passes through your hooks.

```rust
use std::sync::Arc;
use zen_engine::hooks::{HookChain, PreToolUseHook, PreToolUseDecision};
// impl PreToolUseHook for MyPolicy { async fn before_tool_call(...) -> ... }
let mut chain = HookChain::new();
chain.push(Arc::new(MyPolicy));
opts.pre_tool_use_hooks = vec![Arc::new(MyPolicy)]; // Vec<Arc<dyn PreToolUseHook>>
```

Key items: `PreToolUseHook` (async trait, `before_tool_call(tool_name, args, ctx)`),
`PreToolUseDecision::{Allow, Deny(String), Modify(Value)}`, `NoOpHook` (always allow),
`HookChain::{new, push, len, is_empty, run}` (first `Deny` wins; `Modify` feeds the
next hook), `HookedTool::{new, inner}`, `wrap_with_hooks(registry, &chain)`. The
rate-limit policy itself is intentionally not here - it awaits atzenn/zencode#10.

**`read_tracking` (P3).** Defense-in-depth read-before-write at the engine
layer, mirroring the CLI's `ToolContext.read_files` semantics.

```rust
use zen_engine::read_tracking::ReadTracker;
let (tools, tracker) = zen_engine::default_tracked_tools();
// tracker.was_read(&cwd, "src/main.rs") -> bool
```

Key items: `ReadTracker::{new, record_read, was_read, len, is_empty}` (thread-safe,
canonicalized paths), `TrackingReadTool::{new, tracker}` (wraps Read), `ReadGuardedTool`
(wraps Edit/Write; refuses with `"has not been read in this session"`), `GuardMode::{Always, IfExists}`
(Write to a *new* file is allowed), `tracked_tools() -> (ToolRegistry, ReadTracker)`,
`NOT_READ_MSG`.

## `zen-desktop` CLI (engine/src/bin/zen-desktop.rs)

```bash
zen-desktop run "Create hello.txt with: hi" [--model M] [--cwd DIR] [--max-turns N] [--session-dir DIR]
zen-desktop eval [--model M] [--max-turns N]
```

`run` executes against a `:free` model only (non-`:free` models are refused) and saves
the session NDJSON to `<cwd>/.zen/sessions/session-<ts>.jsonl`. `eval` runs 10
small file tasks and prints a metrics table. Env overrides: `ZEN_BASE_URL`,
`ZEN_API_KEY`, `ZEN_MODEL`.

## Validation & safety

| Check | Where | Behavior |
|---|---|---|
| Empty `base_url`, or missing `http(s)://` prefix | `run_task_with_sink` | bail before any network call |
| Empty `model` | `run_task_with_sink` | bail |
| Tool approval | R3 scope guard | no scope → every call denied; `dangerous_allow_all` deprecated |
| Prompt budget (g4) | `prompt_budget` | `ensure_within_budget` fails fast if static prompt > 1200 tokens |
| Read-before-write | `read_tracking` | `ReadGuardedTool` refuses unwritten reads; `Write` to new files allowed |

## Testing

```bash
cd engine && cargo test
```

`tests/fake_llm.rs` runs the full agent loop (HTTP client → agent → real Read/Edit
tools) against a scripted OpenAI-compatible server on localhost - no network, no key.
The 6 tests cover: Read→Edit through real tools, Bash in `cwd`, empty-task/bad-cwd
rejection, server-error surfacing, the v1 event-stream contract (insta snapshot), and
rejection of newer/missing schema versions.

To update the contract snapshot after an intentional schema change:
`cargo insta review` (or `INSTA_UPDATE=always cargo test`), then commit the `.snap` file.
