# Budget Authority (R12)

Per-task hard budgets for the engine: token, tool-call and duration limits
with a **hard stop**. Part of the v0.4.0 "Trust" roadmap.

## Why

The community's unanimous advice for runaway agents is "set hard spend
limits". Every model Zen uses is `:free`, so a budget here is not about
money - it stretches free-tier quotas and kills runaway loops (20–40
iteration death spirals that leave the workspace worse) before they burn
turns.

## How it works

Set `RunOptions.budget` to `Some(TaskBudget { .. })` before `run_task`:

```rust
let mut opts = RunOptions::new(base_url, model, cwd);
opts.budget = Some(zen_engine::budget::TaskBudget {
    max_output_tokens: 50_000,
    ..Default::default()
});
```

Enforcement points inside the existing run loop (no agent core rewritten):

| Dimension | Where | What happens on trip |
|---|---|---|
| `max_input_tokens` / `max_output_tokens` | `BudgetedBackend` wraps the `LlmBackend`: records `usage` after every model call, refuses the next call once over | Task ends immediately as `error_during_execution` |
| `max_tool_calls` | `BudgetedPermissions` wraps the `PermissionChecker`: counts each attempted tool call, **denies** (does not run) calls past the limit | The denial is fed back to the model; the next model call is refused |
| `max_duration_secs` | `BudgetTracker::check` on `Instant::elapsed()` | Same as token trip |

On the **first** trip only, a `budget_exceeded`
`AgentEvent::SystemMessage` is emitted to the session sink, so the NDJSON
session log records exactly what stopped the task. The surfaced message is
always of the form:

```
Task stopped: output token budget 50,000 exceeded (used 50,012)
```

## Defaults

`TaskBudget::default()`:

| Field | Default | Rationale |
|---|---|---|
| `max_input_tokens` | 200,000 | Fits large context windows; trips only on real bloat |
| `max_output_tokens` | 50,000 | Generous for long tasks; catches loops |
| `max_tool_calls` | 100 | Normal tasks use a handful; loops burn hundreds |
| `max_duration_secs` | 600 | 10 minutes wall-clock per task |

Notes:

- A limit of `0` means "none allowed" (e.g. `max_tool_calls: 0` =
  read-only planning). Use `TaskBudget::unlimited()` to disable a dimension.
- Checks use strict `used > limit`: exactly-at-limit usage is allowed.
- Counters saturate instead of wrapping, so a pathological run can never
  overflow back under the limit.
- `RunOptions.budget` defaults to `None` (no enforcement) to preserve
  existing behavior; the Desktop UI is expected to always set it.

## UI contract

Match session-log lines with `"event":"budget_exceeded"` (a `SystemMessage`).
`text` carries the human-readable stop message above. Render it as a stop
banner; the task's `RunResult` will have `is_error = true` with the same
message in `result`.
