# R4 Verified Claims

**Status:** implemented · **Roadmap:** v0.4.0 "Trust" (co-#1 priority)

## Why

"Verify before claiming done" is the #1 rule engineers write into CLAUDE.md files
(19.2% of 10,000 analyzed). 46% of developers distrust AI output accuracy
(Stack Overflow 2025), and the #1 individual frustration is AI being
"almost right, but not quite" (66%). Verified Claims makes every agent assertion
a first-class, auditable object: the UI can show *"3 verified · 1 unverified"*
instead of asking the user to take the agent's word for it.

## Concepts

| Type | Meaning |
|---|---|
| `Claim` | One agent assertion: `{ id, kind, statement, evidence, status }` |
| `ClaimKind` | `TestRun` · `FileWritten` · `CommandExecuted` · `BuildPassed` · `ExternalFact` |
| `Evidence` | `{ kind, detail, verified }` - `verified=true` supports, `false` refutes |
| `ClaimStatus` | `Unverified` → `Verified` / `Contradicted` |
| `ClaimCounts` | `{ verified, unverified, contradicted }` for the SessionEnd summary |

**The one hard rule: a claim with no evidence is NEVER auto-marked verified.**
Status transitions:

- no evidence → `Unverified`
- ≥1 supporting evidence, none refuting → `Verified`
- ≥1 refuting evidence → `Contradicted` (later support does not erase a refutation)

## Usage

```rust
use std::sync::Arc;
use zen_engine::claims::{ClaimKind, ClaimRegistry, ClaimingSink, Evidence};

// 1. Wrap your sink: assistant assertions become claims automatically.
let registry = Arc::new(ClaimRegistry::new());
let sink = Arc::new(ClaimingSink::new(inner_sink, registry.clone()));

// 2. Run as usual.
let result = zen_engine::run_task_with_sink(&opts, task, sink).await?;
// ... or use the convenience wrapper:
let (result, registry) =
    zen_engine::run_task_with_claims(&opts, task, inner_sink).await?;

// 3a. Heuristic evidence is attached automatically for Bash test/build
//     tool results (see `outcome_from_output`).

// 3b. Authoritative evidence: the verify loop (PR #32) maps its report.
registry.attach_verify_evidence(claim_id, report.passed, report.exit_code, &report.output);

// 3c. Manual registration for things the detector can't see.
let id = registry.register_claim(ClaimKind::ExternalFact, "API returns 200 for /health");
registry.attach_evidence(id, Evidence::new("tool_result", "curl: 200 OK", true));

// 4. SessionEnd summary for the UI.
let counts = registry.counts(); // ClaimCounts { verified, unverified, contradicted }
recording_sink.emit_session_end_with_claims(&result, &counts);
```

## Detection heuristics

`detect_claims` fires only on explicit completion assertions ("tests pass",
"build succeeds", "file written", "command succeeded") - never on plans or
intentions. `ExternalFact` is deliberately NOT auto-detected.

`ClaimingSink` additionally watches `ToolResult` events: for Bash invocations
that look like `cargo test` / `pytest` / `npm test` (→ `TestRun`) or
`cargo build` / `npm run build` (→ `BuildPassed`), it attaches the tool output
as evidence to the newest still-unverified claim of that kind. Pass/fail is
decided by output markers (`test result: ok`, `0 failed` vs `failed`, `error`,
`panicked`); failures are checked first so "1 passed, 2 failed" reads as a
refutation.

These heuristics are a convenience layer, not the source of truth: the verify
loop's `attach_verify_evidence` is authoritative and should be preferred
wherever it runs.

## Merge notes

- `SessionEnd` + `emit_session_end_with_claims` are vendored from PR #4
  (`lead-sessionend`) and extended with `claims_verified/unverified/contradicted`.
  When both PRs merge, keep PR #4's fields AND the claim fields - exactly one
  `SessionEnd` variant must survive.
- The verify-loop wiring (`attach_verify_evidence` ← `VerifyReport`) activates
  when PR #32 merges; until then the bridge is tested standalone.
