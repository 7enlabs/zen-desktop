# Convergence Guard (R6)

> v0.4.0 "Trust" roadmap. Detects repair loops and forces a re-plan instead of
> silently burning tokens.

## Why

Runaway token burn is the most universal complaint about coding agents:

- A Cline user burned **$187 overnight** in one stuck session.
- A Roo Code user burned **$47 in a single afternoon** from a loop.
- Repair loops of 20–40 iterations "leave the workspace in a worse state"
  (re-reading files until a 40k-token task consumes 180k).

Zen is free-only, so a loop burns *quota and time* instead of money - but the
frustration is identical. The convergence guard stops the loop early and makes the
agent try a different approach.

## How it works

`engine/src/convergence.rs` provides `LoopDetector`, driven by the tool loop:

| Call | When |
|---|---|
| `record_edit(path)` | After every Write/Edit tool call |
| `record_verify(passed, error)` | After every verify/test outcome |
| `record_stagnant_round()` | After a tool round that changed no files |

Three patterns are detected (defaults in `ConvergencePolicy`):

1. **Same file, failing edits** - the same file edited ≥3 times with a failing
   verify between edits.
2. **Repeating error** - the same normalized error signature (first line,
   lowercased, numbers folded) ≥3 times in a row.
3. **Stagnation** - ≥5 consecutive tool rounds with no file changes.

When a pattern fires:

1. `check()` returns the `LoopPattern` **exactly once** (the task must stop).
2. The caller records a `loop_detected` system event in the session log via
   `emit_loop_detected()` - same schema-preserving pattern as `verify_attempt`
   (PR #32): `AgentEvent::SystemMessage { event: "loop_detected", text: <JSON> }`,
   no `DesktopEvent` change, no schema bump.
3. The caller hands the agent the forced re-plan prompt from `replan_prompt()`:
   what was tried, what failed, and the instruction to propose a **different**
   approach (not retry the same edit). If the caller passes a checkpoint id from
   PR #31's `CheckpointManager`, the prompt mentions the restore point.
4. After re-planning, the caller calls `reset()` to start tracking fresh.

## Integration notes

- `convergence.rs` is self-contained: it does **not** depend on `verify.rs`
  (PR #32) or `checkpoints.rs` (PR #31). The verify loop calls `record_verify`;
  the checkpoint id is passed through as an `Option<&str>`. This keeps the merge
  order flexible.
- A passing verify resets all counters for the edited paths - success breaks
  every loop pattern by definition.
- Thresholds are configurable via `ConvergencePolicy`; `validate()` rejects zeros.

## Threshold rationale

3 attempts matches the community's "max 3 rounds" review-convergence rule and the
verify loop's default `max_attempts = 3` (PR #32): if an approach hasn't worked
after 3 tries with fresh feedback each time, more retries are wishful thinking.
5 stagnant rounds catches "busy but stuck" agents (endless reads/probes) without
flagging legitimate investigation.
