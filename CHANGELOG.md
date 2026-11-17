# Changelog
All notable changes follow [Keep a Changelog](https://keepachangelog.com/) and [SemVer](https://semver.org/).

## [Unreleased]

### Changed (2026-10-18, ZD Leader review batch 5; review: docs/reviews/2026-10-18-lead-branches.md)
- build: ZenCode CLI crates bumped from rev `04b742e` to `250e8d4` (CLI security fixes) in `engine/` and `desktop/src-tauri/`.
- engine/desktop: the Tauri app and the `zencode` binary declare the working directory as the R3 scope instead of setting `dangerous_allow_all`; `zencode` canonicalizes `--cwd` so relative paths stay in scope.
- engine: `ipc_bridge::BridgeSink::to_ndjson` returns `anyhow::Result<String>` instead of panicking.
- docs: release gate renumbered to `RELEASE-GATE-v0.3.0.md` (v0.3.0 "Foundation + Trust"; the v0.4.0 file is removed); README rewritten for the Desktop app; `docs/engine-api.md` updated for GitLab and the R3 scope; `PLAN.md` marked superseded by the final roadmap.

### Added (2026-10-18, ZD Leader review batch 4; review: docs/reviews/2026-10-18-lead-branches.md)
- engine: `smart_router` (R10) - pure free-only model router with fallback chain and health tracking; BYOK only as explicit opt-in.
- engine: `telemetry` (R14) - zero-telemetry guard and auditable network log (`RunOptions.telemetry_log`); desktop Network tab (`desktop/src/network.js`).
- engine: `sovereign` (R5) - loopback-only inference policy, preflight and hash-chained attestation log; `desktop/src/sovereign.js`; adds `sha2`.
- engine: `config_import` - one-click import of external agent configs.
- desktop: R8 60-second onboarding (`desktop/src/onboarding.js`, 19 node tests).
- docs: `RELEASE-GATE-v0.4.0.md`; CLI hand-off list of parked agent-core branches in `docs/upstream-proposals.md`.

### Changed (2026-10-18, OCR review M1–M3 via `lead/ocr-fixes`)
- engine: **tool calls are denied by default**; set `RunOptions::dangerous_allow_all` (or declare an R3 scope) to run tools. The `zencode` binary, the g2 smoke example and the Tauri app opt in explicitly.
- engine: `base_url` / `model` are validated (non-empty, http(s) scheme) before the cost firewall runs.
- engine: `RecordingSink::to_ndjson` returns `anyhow::Result<String>` instead of panicking.

### Added (2026-10-18, R15 signed releases - ZD Leader `lead/signed-releases-r15`; review: docs/reviews/2026-10-18-r15-signed-releases.md)
- release: minisign-signed releases - `tools/sign-release.sh`, `tools/verify-release.sh`, `docs/signed-releases.md`, engine `release_signing` (offline verification via `minisign-verify`, trusted-comment binding, updater key validation); release public key `docs/keys/zencode-release.pub` (key id `A459EE7B787B86FF`) wired into the Tauri updater; GitLab release endpoint added.

### Changed (2026-10-18, ZD Leader branch review: docs/reviews/2026-10-18-lead-branches.md)
- build: `zen-core`/`zen-agent`/`zen-tools` are fetched from the GitLab mirror `https://gitlab.com/atzenn/zencode.git` (rev `04b742e`); the desktop crate's `zen-core` is aligned from `3a09220` to `04b742e` so only one copy is built.

### Added (ZD Leader branches merged 2026-10-18)
- docs: `docs/e2e/` sanitized E2E transcripts, updated `docs/engine-api.md`, `docs/README.md` index.
- docs: research D1–D11, P5 task schema, ACP research, upstream proposals, troubleshooting, release checklist, platform matrix, dependency audit; `deny.toml`; g2 smoke example; CLI rev bump to `04b742e`.

### Added (ZD Leader MRs !1–!6, merged 2026-10-18; review: docs/reviews/2026-10-18-lead-mrs.md)
- engine: `cost_firewall` (R1) - rejects non-`:free`, non-allowlisted models before any network call; `SpendMeter`.
- engine: `claims` (R4) - Verified Claims registry, `ClaimingSink`, `run_task_with_claims`; `DesktopEvent::SessionEnd` with claim counts.
- engine: `convergence` (R6) - loop detection and forced re-plan guard (not yet wired into runs).
- engine: `budget` (R12) - opt-in per-task hard budget (`RunOptions.budget`); adds `async-trait`.
- engine: `scope` (R3) - opt-in declared-scope guard (`RunOptions.scope`) + `desktop/src/scope.js` approval card.
- desktop: R2 time-travel timeline - `checkpoints`, `session_store`, `ipc_bridge`, `timeline` modules and the Tauri 2 shell (`desktop/src-tauri`, `desktop/src`).

### Fixed
- engine: budget checker now wraps the scope guard instead of being shadowed by it; `SessionEnd` handled in IPC; CI-safe `Instant` in a budget test; checkpoint shadow repo keeps CRLF bytes (`core.autocrlf=false`).

### Added
- engine: `events` - versioned (v1) UI event schema, `RecordingSink`, `run_task_with_sink`, NDJSON parser rejecting unknown versions; insta contract snapshot.

### Added
- `engine/` (zencode_engine): `run_task()` runs the ZenCode CLI agent loop (zen-agent @3a09220) with Read/Edit/Write/Bash; 4 integration tests against a fake OpenAI-compatible server on localhost (g1).

### Added
- core: `JsonlLog` (append-only JSONL, tolerates a truncated last line, trims a corrupt tail) and `dedupe_instructions` (AGENTS.md/CLAUDE.md with identical content are injected only once).
- New std-only crate `core/` (`zencode_core` 0.1.0): agent seams `Llm`, `Tool`, `Sandbox` (fail-closed `NoSandbox`, honest `Isolation`), `SessionLog` (append-only `MemoryLog` with `replay_context` and `fork`) and a minimal loop `run_turn`; invariant "everything the model sees is logged". 6 integration tests.
- CI lint and test jobs now run a crate matrix (`rebuild`, `core`).
- `PLAN.md` roadmap for the agent core.
- `--cow-color aurora`: diagonal hue gradient, 24-bit when `COLORTERM=truecolor|24bit`, else xterm 256-colour.
- `--cow-color <red|green|yellow|blue|magenta|cyan|white|gray|bold|rainbow>` and `--color=auto|always|never` (ZenCode extension). Default `auto`: colour only on a terminal, honours `NO_COLOR` and `CLICOLOR_FORCE`; piped output stays byte-identical to cowsay@1.6.0. Windows enables VT processing via `SetConsoleMode` (std-only FFI).
- Commit `Cargo.lock`; CI builds/tests with `--locked`.
- New CI lint job: `cargo fmt --check` + `cargo clippy -D warnings` for Linux, Windows and macOS targets.
- Code formatted with rustfmt; clippy fixes (`is_some_and` / `is_none_or`).

## [0.2.0] - 2026-10-16
### Added
- `-W auto` now falls back to the real terminal width (Unix `ioctl(TIOCGWINSZ)`, Windows `GetConsoleScreenBufferInfo`), std-only FFI.
- Release workflow: tag `v*` → test + build binaries for Linux x86_64, macOS arm64, Windows x86_64 → GitHub Release.

## [0.1.0] - 2026-10-16
### Added
- Std-only Rust rebuild of npm `cowsay@1.6.0` (byte-for-byte default output, 21/21 golden cases).
- Library API `cowsay_bubble::say(text, &Options)`, `Wrap::{None, Hard, Words}`, `auto_width()`.
- CLI: `-W N|auto`, `-n`, `--word-wrap`, 190 embedded cowfiles, `-f`, `-r`, `-l`, modes `-bdgpstwy`, `-e`, `-T`, `--think`/`cowthink`.
- CI matrix Ubuntu/macOS/Windows.
