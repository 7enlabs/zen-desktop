# Review: all ZD Leader branches (2026-10-18)

Reviewer: Z App. First pass on `gitlab/main` @ `9548bb4`; batch 4 on `0ec7db2`; batch 5 (this update) on `9ba91f7`.
Companion review of the MRs: `docs/reviews/2026-10-18-lead-mrs.md`.

## Summary

99 ZD Leader branches on GitLab (79 in the first pass, 13 in batch 4, 7 added in batch 5 incl. `lead/gitlab-ci`):

| Verdict | Count | Meaning |
|---|---|---|
| merged | 41 | On `main` (`git merge --no-ff` or `cherry-pick -x`, original commits kept). |
| superseded | 20 | Content already on `main` or replaced by a newer branch. Nothing to merge. |
| rejected | 7 | Will not be merged as-is (reason below). |
| needs-CLI | 22 | Agent-core behaviour; belongs upstream in `atzenn/zencode` (pointers in `docs/upstream-proposals.md`). |
| pending | 9 | Needs a split or a port onto the current shell by ZD Leader (instructions below). |

### Batch 5 (2026-10-18, on `9ba91f7` after MR !7)
Merged (`--no-ff`, with one review-fix commit):
- `cli-250e8d4-clean` (7ece972): single rev-bump commit 04b742e → 250e8d4 (engine). Review fix: the desktop
  crate pinned `zen-core` at 04b742e, which would build two `zen-core` copies; aligned to 250e8d4.
  GitLab pipeline 2959550217: fmt, clippy, test, release-key-check green.
- `ocr-followup` (70ab59a): the Tauri app and the `zencode` binary declare the working directory as the R3
  scope instead of `dangerous_allow_all`; `ipc_bridge::BridgeSink::to_ndjson` returns `Result`.
  Review fix: `ScopeGuard` compares paths lexically, so a relative `zencode run --cwd .` denied every write
  (reproduced with a throwaway test: relative root → `in_scope("src/a.rs") == false`); the binary now
  canonicalizes the directory. The Tauri hunk (8 lines) uses only public engine API with the same shape as
  the binary; the Tauri crate itself is still not built here.
- `release-gate-v030` (2635216): gate report renumbered to v0.3.0. Review fix: `RELEASE-GATE-v0.4.0.md`
  removed (same report), Vietnamese exit-criteria rows translated, CLI note updated for 250e8d4.
- `docs-trust-followup` (f6f3871): README rewrite, `docs/engine-api.md` on GitLab + R3 scope, PLAN.md marked
  superseded. Review fix: `ScopeDecl::for_dir` does not exist, replaced with the struct literal.
Pending: `sec-docs-security-md`, `ui-port-notifications` (see table). Superseded: `cli-250e8d4`
(by `-clean`), `notifications` (by `ui-port-notifications`).
No new pushes on the other pending branches since batch 4.
Still open: `PLAN.md` body (legacy, superseded) is Vietnamese; translate or archive it in a follow-up.
Verification (`engine/`, aarch64 Linux, after all merges): `cargo fmt --check` clean,
`cargo clippy --locked --all-targets -- -D warnings` clean, `cargo test --locked` **165 passed, 0 failed**
(154 lib + 3 budget + 8 fake_llm; 1 doc-test ignored). Desktop JS not touched.

### Batch 4 (2026-10-18, on `0ec7db2`)
Merged: `ocr-fixes`, `config-import`, `sovereign-mode` (R5 commit only), `smart-router-r10`,
`onboarding-r8`, `zero-telemetry-r14`, `release-gate-v040`.
Decided: `model-providers`, `cloudflare-tunnel`, `design-to-code`, `speckit-init` rejected (reasons in the table).
**Behaviour change (OCR M1):** `run_task*` now denies every tool call unless `RunOptions::dangerous_allow_all`
is set or a scope is declared. Callers in this repo opt in explicitly; the Tauri app should move to the
R3 scope/approval flow. `ipc_bridge::…::to_ndjson` still uses `expect` (follow-up, same pattern as M3).
Verification (`engine/`, aarch64 Linux): `cargo fmt --check` clean, `cargo clippy --locked --all-targets -D warnings`
clean, `cargo test --locked` **165 passed, 0 failed** (154 lib + 3 budget + 8 fake_llm; 1 doc-test ignored);
`node --test desktop/src/onboarding.test.js` 19/19. Tauri crate not built here (needs webkit2gtk).

### Dependency source fix (this batch)
GitHub is unreachable for this project, so CI could not fetch `zen-core` / `zen-agent` / `zen-tools`.
Commit `build: fetch zencode from GitLab mirror`:
- `engine/Cargo.toml` and `desktop/src-tauri/Cargo.toml` now use `https://gitlab.com/atzenn/zencode.git`, rev `04b742e`.
- `desktop/src-tauri` pinned `zen-core` at `3a09220` while the engine used `04b742e`: two incompatible
  `zen-core` copies (`ForwardingSink` passes a `zen_core::AgentEvent` into the engine's `BridgeSink`).
  Aligned to `04b742e`; the stale desktop `Cargo.lock` was refreshed (also picks up `async-trait` from !4).
- Not built here: the Tauri crate (needs webkit2gtk; not in the CI matrix). `cargo metadata` resolves cleanly.

### Verification (`engine/`, aarch64 Linux, after all merges)
- `cargo fmt --check`: clean
- `cargo clippy --locked --all-targets -- -D warnings`: clean
- `cargo test --locked`: **91 passed, 0 failed** (82 lib + 3 `tests/budget.rs` + 6 `tests/fake_llm.rs`)

## All branches

| Branch | GitHub PR | Verdict | Reason |
|---|---|---|---|
| `lead/acp-research` | #38 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/claude-learnings` | #4 | merged | Reference-agent learnings + snapshot scrub; merged earlier today. |
| `lead/d1-stream-emitter` | #13 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d10-notifications` | #25 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d11-background-service` | #24 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d2-ipc-design` | #15 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d3-plugins-design` | #18 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d4-session-design` | #14 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d5-checkpoints-design` | #20 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d6-terminal-design` | #19 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d7-model-picker` | #22 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d8-rategate-ui` | #28 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/d9-queue-steer` | #21 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/dep-audit` | #16 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/desktop-proposals` | #2 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/docs-index` | - | merged | docs/ index (batch 3). Status column still says "PR #N" for docs now on main - refresh later. |
| `lead/e2e-transcripts` | #11 | merged | Sanitized E2E transcripts (docs only), clean merge (batch 3). |
| `lead/engine-api-update` | - | merged | Engine API guide incl. new modules (docs only), clean merge (batch 3). Unmerged modules are labelled with their PR. |
| `lead/g2-smoke` | #3 | merged | g2 smoke example; merged earlier today. |
| `lead/multiplatform` | - | merged | Docs + cross-compile script; merged earlier today. |
| `lead/p5-task-schema` | #7 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/release-checklist` | #42 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/rev-bump-04b742e` | #1 | merged | CLI rev bump to 04b742e + clippy fix; merged earlier today. |
| `lead/supply-chain-deny` | - | merged | cargo-deny config + license metadata; merged earlier today. |
| `lead/troubleshooting-guide` | #27 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/upstream-proposals` | #10 | merged | Docs/research only; merged earlier today (batch 1–2). |
| `lead/budget-authority` | - | superseded | Landed via MR !1–!6. |
| `lead/chat-ui` | - | superseded | Code vendored into main by MR !6 (Tauri shell / chat UI / engine wiring). |
| `lead/convergence-guard` | - | superseded | Landed via MR !1–!6. |
| `lead/d4-session-index` | - | superseded | Module already on main via MR !6 (session_store.rs). |
| `lead/d5-checkpoints` | - | superseded | Module already on main via MR !6 (checkpoints.rs, with autocrlf fix). |
| `lead/engine-api-guide` | #29 | superseded | Older version of docs/engine-api.md; replaced by engine-api-update. |
| `lead/engine-wireup` | - | superseded | Code vendored into main by MR !6 (Tauri shell / chat UI / engine wiring). |
| `lead/ipc-bridge` | #43 | superseded | Module already on main via MR !6 (ipc_bridge.rs, with SessionEnd/Unknown arms). |
| `lead/scope-guard` | - | superseded | Landed via MR !1–!6. |
| `lead/tauri-shell` | - | superseded | Code vendored into main by MR !6 (Tauri shell / chat UI / engine wiring). |
| `lead/undo-timeline-ui` | - | superseded | Landed via MR !1–!6. |
| `lead/verified-claims` | - | superseded | Landed via MR !1–!6. |
| `lead/zero-cost-firewall` | - | superseded | Landed via MR !1–!6. |
| `lead/changelog` | - | rejected | Lists unmerged PRs under 0.3.0; CHANGELOG is maintained on main instead. |
| `lead/merge-helper` | - | rejected | Built on gh/GitHub merge queue; GitHub unavailable and not our workflow. |
| `lead/ocr-minor-fixes` | - | rejected | Touches frozen core/ (and rebuild/). |
| `lead/agent-skills` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/approval-flow` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/compaction` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/g3-rategate` | #33 | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/integration-test` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/linux-sandbox` | - | needs-CLI | OS sandbox belongs in zen-tools/CLI; propose upstream. |
| `lead/macos-sandbox` | - | needs-CLI | OS sandbox belongs in zen-tools/CLI; propose upstream. |
| `lead/memory-polish` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/p2-pretooluse-hook` | #8 | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/p3-read-tracking` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/parallel-tools` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/plan-mode` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/repo-map` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/smart-router` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/user-learning` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/verify-loop` | - | needs-CLI | Agent-loop behaviour; propose upstream to atzenn/zencode, keep Desktop as a thin wrapper. |
| `lead/wasmtime-sandbox` | - | needs-CLI | OS sandbox belongs in zen-tools/CLI; propose upstream. |
| `lead/windows-sandbox` | - | needs-CLI | OS sandbox belongs in zen-tools/CLI; propose upstream. |
| `lead/atomic-writes` | - | needs-CLI | Atomic writes for Write/Edit tools belong in zen-tools; listed in docs/upstream-proposals.md. |
| `lead/cli-v010` | - | superseded | Replaced by the newer rev bump on `lead/cli-250e8d4`. |
| `lead/cloudflare-tunnel` | - | rejected | Decision: "ZenCode Cloud" public mode is outside Desktop scope; exposing an agent that runs shell tools needs its own auth/threat model, and Cloudflare sees plaintext at its edge. Revisit as a separate Cloud project. |
| `lead/config-import` | - | merged | Batch 4. Only `lib.rs` module-list conflict. |
| `lead/cost-leaderboard` | - | pending | Built on zero-cost-benchmark; rebase after that is split. |
| `lead/design-to-code` | - | rejected | Decision: not wired: depends on the `LlmClient` from model-providers (rejected) and `slash.rs` (not on main). Resubmit small: deterministic codegen + vision via a `:free` model routed by `smart_router` (Vision). |
| `lead/engine-benchmarks` | #37 | pending | Benchmarks `prompt_budget` (needs-CLI). Resubmit benches for Desktop modules only (+ criterion dev-dep). |
| `lead/g4-prompt-budget` | #6 | needs-CLI | Prompt budgeting is agent-core; the bundled spec-kit scaffold is rejected (see speckit-init). |
| `lead/model-providers` | - | rejected | Decision: 4,700 lines; vendors a second `cost_firewall.rs`; adds Anthropic/paid providers and subscription OAuth sign-in with ban-risk warnings. Conflicts with the 0-cost policy. A future small MR may add a local (Ollama) provider or opt-in BYOK using the existing firewall and `smart_router::CostMode`; free-only stays the default. |
| `lead/notifications` | - | superseded | Batch 5: replaced by the port on `lead/ui-port-notifications`. |
| `lead/ocr-fixes` | - | merged | Batch 4. Rebased by hand: M1 deny-by-default (`dangerous_allow_all` opt-in; a declared R3 scope still uses ScopeGuard; zencode bin, g2 smoke, Tauri app and tests opt in explicitly), M2 input validation moved before the cost firewall, M3 `to_ndjson` returns Result. |
| `lead/rategate-ui-wireup` | - | pending | Same as notifications: port only `rategate.js` onto the current shell. RateGate policy itself is needs-CLI (g3). |
| `lead/security-sweep` | - | pending | 32 commits / 62 files mixing web app, companion, docs and engine. Split into single-purpose MRs. |
| `lead/skills-index` | - | needs-CLI | Depends on agent-skills (needs-CLI). |
| `lead/sovereign-mode` | - | merged | Batch 4. Cherry-picked only the R5 commit (b0b8bbe); its base commit b3a49a6 (benchmark harness bundling CLI modules) not taken. Adds `sha2`. |
| `lead/speckit-init` | - | rejected | Decision: ~5,200 lines of third-party spec-kit scaffold (`.specify/`, `.agent/`). Keep out of tree; contributors can run `specify init` locally. |
| `lead/terminal-tab` | - | pending | Same as notifications: port only `terminal.js`/`terminal.css` and the `main.rs` command onto the current shell. |
| `lead/zero-cost-benchmark` | - | pending | Bundles needs-CLI modules (`repo_map`, `verify`, `prompt_budget`, `atomic_write`). Split: resubmit the cost harness only, against Desktop modules. |
| `lead/zero-telemetry` | - | superseded | Replaced by `lead/zero-telemetry-r14` (merged). |
| `lead/signed-releases-r15` | - | merged | R15 signed releases; merged earlier today with a review fix (0ec7db2). |
| `lead/smart-router-r10` | - | merged | Batch 4. Pure free-only model router (no network I/O); FreeOnly default, BYOK opt-in only for ids the user configures. |
| `lead/onboarding-r8` | - | merged | Batch 4. R8 onboarding UI; `node --test` 19/19 pass. |
| `lead/zero-telemetry-r14` | - | merged | Batch 4. R14 zero-telemetry guard + network log; `lib.rs`/`index.html` conflicts resolved (union). |
| `lead/release-gate-v040` | - | merged | Batch 4. Release gate report (docs). |
| `lead/zencode-run` | - | merged | Already an ancestor of main. |
| `lead-sessionend` | - | merged | Already an ancestor of main (MR source branch). |
| `lead/cli-04b742e` | - | superseded | main already pins 04b742e; newer bump is cli-250e8d4. |
| `lead/gitlab-dep-src` | - | superseded | Same fix landed as `build: fetch zencode from GitLab mirror` (3aa1f70). |
| `lead-session-store` | - | superseded | `session_store.rs` already on main via MR !6. |
| `lead-verify-loop` | - | needs-CLI | Same as `lead/verify-loop`. |
| `lead/cli-250e8d4` | - | superseded | Batch 5: replaced by `lead/cli-250e8d4-clean` (merged). |
| `lead/streamjson-v1` | - | pending | Overlaps the Desktop stream-json v2 adaptation that waits for CLI f703d24 on GitLab; reconcile the two parsers then merge one. |
| `lead/gitlab-ci` | - | merged | GitLab CI (fmt, clippy, test, cargo-deny, release-key-check); merged 09:25 with review fix 239bc29 (91a7907). |
| `lead/cli-250e8d4-clean` | - | merged | Batch 5. One clean rev bump to 250e8d4; review fix aligns the desktop `zen-core` pin. Pipeline green (fmt/clippy/test). |
| `lead/ocr-followup` | - | merged | Batch 5. App and binary use the R3 scope (cwd) instead of `dangerous_allow_all`; `BridgeSink::to_ndjson` returns Result. Review fix: canonicalize `--cwd` (relative paths denied every write). |
| `lead/release-gate-v030` | - | merged | Batch 5. Gate report renumbered to v0.3.0; review fix removes the v0.4.0 copy and translates the Vietnamese rows. |
| `lead/docs-trust-followup` | - | merged | Batch 5. README / engine-api / PLAN header refresh; review fix for the non-existent `ScopeDecl::for_dir`. |
| `lead/sec-docs-security-md` | - | pending | `SECURITY.md` lists open decisions and "what the sweep fixed" for code that is not on main (companion, Telegram, cloud, skill catalog, automations). A root `SECURITY.md` should be the vulnerability-reporting policy. Resubmit as `docs/security/r7-open-decisions.md`, scoped to code on main, together with the split `security-sweep` MRs. |
| `lead/ui-port-notifications` | - | pending | The requested port (new files + small `main.rs`/`index.html` hunks), good shape. Tauri crate cannot be built here and CI does not build it; `register_actions` uses notification action types, which may be mobile-only in `tauri-plugin-notification` 2. Needs a Tauri build log (desktop target) in the MR. |

## Notes for ZD Leader

**Next work, in order**
1. ~~`cli-250e8d4`~~ done (batch 5).
2. Port `terminal-tab`, `rategate-ui-wireup` onto the current shell (notifications port is in review): new files + small
   `main.rs`/`index.html` hunks only, never re-vendor shell files or icons. Show a Tauri build in the MR.
3. Split `zero-cost-benchmark` (cost harness only, no CLI modules) → then `cost-leaderboard`, `engine-benchmarks`.
4. Split `security-sweep` into small MRs.
5. ~~Move the Tauri app to the R3 scope flow~~ done (batch 5); next: approval card UI for `needs_approval` and scope expansion.
6. Refresh the status column in `docs/README.md`. Attach a Tauri build log to `ui-port-notifications`.

**Rules**
- At most 5 new MRs and 5 issues per day, at most 20 pushes per day, per account (ZD Leader and Z App share
  the budget). Writes are sequential; on 403/429 stop all writes for at least 1 hour.
- One MR per milestone; rebase onto `main` before opening; run fmt + clippy `-D warnings` + tests locally.
- Everything in the repo is in English (code, docs, commit messages, MR text).
- Agent-core behaviour (loop, tools, compaction, sandbox, hooks, rate gate, memory, routing) is proposed
  upstream to the CLI `atzenn/zencode`, not reimplemented in Desktop. `core/` stays frozen.
- Dependencies come from `https://gitlab.com/atzenn/zencode.git` at one pinned rev for every crate.

**When GitHub comes back**
- Push `main`: PRs whose commits are on `main` close as merged automatically.
- Close rejected / needs-CLI / superseded PRs manually, at most 10 per day, each with a one-line pointer
  to this document. Use half the normal limits during the first week.
