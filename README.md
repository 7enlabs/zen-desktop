# Zen Desktop

The AI coding agent that cannot bill you - structurally free-only. A Tauri 2 desktop app
built on the [Zen CLI](https://gitlab.com/atzenn/zencode) agent core via git dependencies.

## Architecture

- **`engine/`** - crate `zen_engine`: thin assembly layer over CLI crates (`zen-agent`,
  `zen-tools`, `zen-core` from `gitlab.com/atzenn/zencode` at a pinned rev). Owns the
  Trust stack, the versioned event schema (`DesktopEvent`), and `run_task()` / `zen-desktop run`.
- **`desktop/`** - Tauri 2 app (Rust backend + web UI): session control room, model picker,
  onboarding, network log, scope/approval UI.
- **`core/`** - frozen pilot crate (`zen_core`); kept for reference, do not extend.
- **`rebuild/`** - cowsay pilot (byte-for-byte rebuild); kept for reference.

## Trust stack (merged on main)

| ID | Module | What it does |
|----|--------|--------------|
| R1 | `engine/src/cost_firewall.rs` | Structural zero-cost firewall: the engine can never call a paid model |
| R2 | `engine/src/timeline.rs` | Time-travel / undo-everything timeline model for the UI |
| R3 | `engine/src/scope.rs` | Ask-first + scope guard: agent declares files/dirs before acting |
| R4 | `engine/src/claims.rs` | Verified claims: agent assertions require evidence |
| R6 | `engine/src/convergence.rs` | Convergence guard: detects repair loops, forces re-plan |
| R12 | `engine/src/budget.rs` | Hard budget authority: per-task token/tool-call/duration budgets |

Also: R15 signed releases (`engine/src/release_signing.rs`, minisign verify), R5 sovereign
mode, R8 onboarding, R10 free-only router, R14 zero-telemetry.

## Build & test

```bash
cd engine && cargo test          # engine tests (165+)
cd engine && cargo clippy --all-targets -- -D warnings
cd engine && cargo fmt --check
```

CI (`.gitlab-ci.yml`): fmt, clippy `-D warnings`, `cargo test --locked`, `cargo deny`,
release-key check - runs on push/MR.

GitHub CI (`.github/workflows/`): PR Agent free-model review (Route A, same config
as CLI: `deepseek-v4.1-flash` with free fallbacks, auto-approval
off) runs on PR opened, reopened, and ready for review only.

## Roadmap

The authoritative roadmap is **Zen Desktop - Roadmap FINAL** (Notion, Zen Hub,
written by Z App, 2026-10-18). Milestones: v0.3.0 "Foundation + Trust" → v0.4.0
"Stream, Freedom & Hardening" → v0.5.0 "Control Room & Proof" → v1.0.0 "Stable & Open".

Legacy files kept for reference: `PLAN.md` (old), `RESEARCH.md`, `RELEASE-GATE-v0.3.0.md`.

## Rules

- **Never use `main`.** All work on `dev/*` (developer) or `lead/*` (leader) branches, opened as MRs for Z App's review.
- **Free-only.** Only `:free` / local models in development and testing. Never spend money.
- **English** in repo (commits, docs, MRs); Vietnamese in reports/Notion.
- Release gate: before tagging, run context7 → ocr → e2e → rea → bughunter; tag only when all clean.
