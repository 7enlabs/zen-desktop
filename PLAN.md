# PLAN - ZenCode Desktop

> **SUPERSEDED.** The authoritative roadmap is **ZenCode Desktop - Roadmap FINAL**
> (Notion, ZenCode-Hub, written by Z App, 2026-10-18). This file is kept for history.
> Milestones: v0.3.0 "Foundation + Trust" → v0.4.0 "Stream, Freedom & Hardening" →
> v0.5.0 "Control Room & Proof" → v1.0.0 "Stable & Open".

Rules: each session performs ONE step (≤ ~1 hour), record the proof; 2 failures at the same step → record in "Failures" section and stop that step.
Do not break `rebuild/` (golden parity 21/21). Use only `:free` models, do not spend money.

## Core agent roadmap
| # | Step | Status | Proof |
|---|------|-----------|------------|
| a | Crate `core/` (`zencode_core`, std-only): trait `Llm`, `Tool`, `Sandbox`, `SessionLog` + `run_turn` loop | DONE 2026-10-17 | `cd core && cargo test` (6 loop tests: tool call, "model sees = logged", sandbox deny returns to model, unknown tool + step limit, LLM error logged, resume + fork); CI lint/test matrix includes `core` |
| b | `JsonlLog`: SessionLog append-only JSONL on disk (std-only JSON encode/decode), replay/fork from file; dedupe AGENTS.md/CLAUDE.md by content hash | DONE 2026-10-17 (core/src/jsonl.rs, core/tests/jsonl.rs: 4 tests) | test round-trip write→read→`replay_context` are equal; mid-write error does not corrupt previous lines; 2 files with same content injected only once |
| c | Minimal tools `shell` + `str_replace` (Minimal mode), ask `Sandbox` before any side effect | SUPERSEDED 2026-10-17 → use zen-tools from zencode CLI | test in tempdir: str_replace matches exactly once, errors when 0/≥2 match; NoSandbox refuses |
| d | OpenAI-compatible Client to free channel (`/workspace/tools/free-llm.sh` / relay), only `:free` models | SUPERSEDED 2026-10-17 → use zen-agent client.rs (retry 429 + fallback_models) | refuse models without `:free` suffix; test with localhost mock server |
| e | Sandbox fail-closed (Landlock / sandbox-exec / Job Object), report `Isolation::Partial` honestly | DEFERRED (after CLI integration; CLI already has permissions) | test policy; return Partial instead of Full on unsupported kernels |
| f | Ideas from docs/research/daily-feature-ideas.md (file does not exist yet) | - | - |

When (b)+(c)+(d) are done → agent core can run a real loop → tag `v0.3.0`.

## NEW DIRECTION 2026-10-17: based on ZenCode CLI (Zen's decision)
ZenCode CLI = https://gitlab.com/atzenn/zencode (public, Rust workspace ~27k lines: zen-core, zen-agent, zen-tools, zen-cli, zen-ext, zen-mcp). Already exists: agent loop, OpenAI-compatible client + retry 429/5xx according to Retry-After + `fallback_models`, `edit_match.rs` for flexible file editing, compaction, byte-identical session event log replay, hooks, MCP, sub-agent, background Bash, permissions, headless JSON, CI + cargo-audit. Desktop will NOT rewrite these parts; `core/` (zencode_core) is frozen, kept for reference, not deleted.
The Desktop consumes the CLI crates through a pinned-rev git dependency (currently `3a09220`); rev bumps are deliberate. Only read access to the atzenn repo: core improvements go in a Desktop-side wrapper layer, and proposals are recorded in docs/upstream-proposals.md (send PRs once Write access is granted).

| # | Step | Status | Proof |
|---|------|-----------|------------|
| g1 | Crate `engine/` depending on `zen-agent`, `zen-tools`, `zen-core` (git, pinned rev); run 1 agent loop with mock LLM (localhost server) calling Read + Edit tools in tempdir | DONE (4 tests: Read→Edit via real HTTP to mock server, Bash, 500 error, wrong input) | `cd engine && cargo test` green; CI build engine on 3 OS |
| s1 | Versioned event schema (engine/src/events.rs, SCHEMA_VERSION=1) + insta contract test | DONE 2026-10-17 (CI green 3 OS, 4512c0f) | `cd engine && cargo test`: snapshot event_stream_v1 + refuse new/missing version (6 tests green) |
| g2 | `zencode run "<task>"` calls engine with real free channel (only `:free`), save session | TODO | run 1 small real task, transcript without keys into docs/e2e/ |
| g3 | RateGate: daily quota per provider (OpenRouter :free 50 req/day, errors count) wrapping CLI client, switch model when exhausted | TODO | test quota counting + model switching with mock server |
| g4 | Prompt budget: measure CLI system prompt + tool defs, CI test ≤1,200 tokens (or minimal tool profile) | TODO | test print token count, fail if exceeded |
| g5 | `zencode eval` 10 tasks, table of requests/tokens/429/edit-fail/pass | TODO | run with ≥1 :free model; tag v0.3.0 when ≥5/10 + CI green |
| g6 | Desktop UI (after v0.3.0) | TODO | - |
## Competitor Research Roadmap (2026-10-17, docs/research/competitors-2026-10-17.md §3)
- v0.3.0: (1) `zencode run` + shell/read/str_replace + JSONL; (2) RateGate + ModelChain (429, cap 50 req/day :free, fallback); (3) strict tool-call parser + text-mode fallback (1 bash block) + lenient str_replace (whitespace/CRLF/indent, still exactly 1 match); (4) system prompt + tool defs ≤ 1.200 token (CI test) + trim tool output; (5) `zencode eval` 10 tasks, table of requests/tokens/429/edit-fail/pass. Tag when ≥1 model :free actually done on ≥5/10.
- v0.4.0: multiple tool calls/turn + read-only parallel; compaction; checkpoint/undo + atomic write; verify loop `--until` + plan mode.
- v0.5.0: repo map ≤1k token; hooks; sandbox Linux → macOS; ACP for Zed.

## Design Notes
- `core/` is an independent crate (separate Cargo.lock), not merged into workspace to avoid changing the `rebuild/target` path that release.yml currently uses.
- Package name `zencode_core` (to avoid clash with Rust's `core` crate).
- Log invariant: every message entering context goes through `log::see()` (log first, then push); `run_turn` initializes context with `log.replay_context()` → natural resume/fork.
- Tool / sandbox denial errors are returned to the model as a `tool` message "ERROR: ..."; LLM / log / step limit errors end the turn and are logged as `Event::Error`.

## Infrastructure
- The sandbox may lose `gcc` after a restart → `cargo test` reports "linker `cc` not found". Fix: `sudo -n apt-get update -qq && sudo -n apt-get install -y -qq gcc libc6-dev`.

## Failures
- (none)
- g1 note: Bash tool on Windows CI fails (unknown cause, need detailed logs) → Bash test runs only on Unix; need investigation in g2. Clippy cross-target dropped for engine (ring does not cross-build); native tests on 3 OS still run.

## Status notes (English, 2026-10-19)

### 0.3-7 - R6 convergence guard wired into `run_task` - DONE
- `RunOptions::convergence: Option<ConvergencePolicy>` (default `None` = off).
  `ConvergenceMonitor` observes the agent from outside (sink / backend /
  permission wrappers): first loop → the next tool call is refused with the
  forced re-plan prompt; a second loop → `convergence_stop` event, tools denied,
  the next model call fails with the reason.
- Stagnation fix: a tool round is stagnant only if it made no edit, no passing
  verify (Bash), and produced no tool output not already seen in this run.
  Progress rounds reset the streak (`LoopDetector::record_progress_round`).
  Repeating the same failing command or the same probe is still stagnant.
- Proof: `cd engine && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --locked`
  → 176 passed, 0 failed, 1 ignored (InstaBox, 2026-10-19). Integration tests in
  `engine/tests/convergence.rs`: loop → re-plan → stop; progress stays quiet;
  repeated identical probe still fires `stagnant_rounds`.

### 0.3-5 part 1 - unsigned `cargo tauri build` on Linux (InstaBox) - PASSED
- Built 2026-10-18 in `desktop/src-tauri` (log `~/tauri-build.log` on the box):
  release binary in 2m40s (total 3m15s), `zen-desktop` binary 33.3 MB.
- Bundles: `ZenCode Desktop_0.1.0_amd64.deb` 9.36 MiB,
  `ZenCode Desktop-0.1.0-1.x86_64.rpm` 9.36 MiB,
  `ZenCode Desktop_0.1.0_amd64.AppImage` 109.29 MiB. No compile errors or
  warnings; only linuxdeploy "Not calling strip" notices for bundled libs.
- No `.sig` files were produced (unsigned build). Note: since 0.3-4 set
  `bundle.createUpdaterArtifacts: true`, a local build without
  `TAURI_SIGNING_PRIVATE_KEY` is expected to stop at the updater signing step
  (Tauri behaviour, not yet re-verified here); local smoke builds can pass
  `--config '{"bundle":{"createUpdaterArtifacts":false}}'`.
- Next (part 2): run the AppImage headless (xvfb) smoke test; macOS/Windows
  bundles need CI runners.
