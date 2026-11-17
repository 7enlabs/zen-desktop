# Zen Desktop - architecture & build order (agreed 2026-10-17)

> Canonical plan: the "PLAN-v0.3" page on the shared Notion space Zen Hub (summarised in the last section). Where sections differ, the last section wins.

## 1. App foundation
- Tauri 2 shell calling the `zen-agent` / `zen-core` crates. Official plugins: updater, shell, fs, dialog, store, log, window-state, single-instance, deep-link (`zen://`).
- Pure-Rust alternatives if we drop the webview: GPUI, Iced, egui.

## 2. Connecting to Zen CLI (github.com/atzenn/zencode)
- Either crates via a pinned git dependency, or run `zen` headless with `--output-format stream-json`.
- One versioned schema file for stream-json and the session event log, with contract tests (CLI repo is read-only for us: contract tests live in Desktop).
- ACP (Agent Client Protocol): Desktop can host Zen CLI and other agents; Zen CLI can plug into external editors.
- MCP: reuse `zen-mcp`, add an MCP server manager screen.

## 3. UI components
- xterm.js + `portable-pty`: real terminal.
- CodeMirror 6 (light) or Monaco: view/edit code.
- `similar`: per-hunk diff with accept/reject.
- `gix`: git status, checkpoints/undo.
- `notify`: file watching.
- `tree-sitter`: syntax highlighting, repo map.

## 4. Testing & quality
- cargo-nextest; insta (snapshots of stream-json / UI state).
- tauri-driver (WebDriver) or Playwright / tester-army e2e (local runner: Firefox, as linux-arm64 has no Chromium build): UI tests including break-it checks (empty input, rapid clicks, reload).
- cargo-deny + cargo-audit; cargo-llvm-cov.

## 5. Build & release
- tauri-action: Windows, macOS, Linux builds.
- sccache + Swatinem/rust-cache.
- release-please or git-cliff for the CHANGELOG.
- Code signing costs money → later; ship unsigned builds first.

## 6. Agent tooling
- obra/superpowers, context7: TDD/plan workflow, current Tauri docs.
- Coding rules → repo `AGENTS.md`.
- Team code review and REA + oracle prompts for PR review and CLI parity tests.
- Copy `REVIEW.md` + PR template from Zen CLI.

## 7. Alignment with Zen Hub (read 2026-10-17) - takes precedence
- PLAN-v0.3 steps:
  0. Freeze `core/` (tag `core-pilot-v0`, README marks it experimental).
  1. CLI bridge: run `zen -p --output-format stream-json` (zen release binary, pinned version), parse events line by line. Proof: fake process printing sample stream-json → parser yields the right events; wrong binary version → clear error.
  2. Schema contract: stream-json + event log (`"v":1`) fixtures from Zen CLI in `tests/fixtures/`; changing one field turns the test red; CI green on real fixtures.
  3. Tauri 2 shell (`app/`): single-session chat, streaming text + tool calls; plugins log/store/window-state/single-instance; tauri-driver e2e with a mock model + break-it checks.
  4. Resume/fork (`zen --resume`); show tool results marked "interrupted".
  5. Per-hunk diff view (`similar`); rejecting a hunk leaves the file unchanged.
  6. Embedded terminal (xterm.js + portable-pty); Ctrl-C stops long commands.
  7. Cross-platform CI with tauri-action (unsigned) + cargo-deny/audit + rust-cache.
- v0.3.0 milestone: steps 1–5 done and one real `:free` model completes a file-editing session through Desktop. Later (v0.4–0.5): ACP, MCP manager, auto-update, tree-sitter repo map, fail-closed sandbox, code signing.
- `engine/` (crate dependency) stays as a secondary path/test harness; the main path is the stream-json bridge.
- CLI bugs/missing features: open an issue on atzenn/zencode labelled `desktop` (no temporary patches in Desktop); also log in `docs/upstream-proposals.md`.
- Notion: never edit CLI-team pages. Add new subpages under Zen Hub titled "Desktop - …"; the CLI team reads them nightly at 21:30.
- Repo language: everything committed to GitHub is in English.

## 8. Review 2026-10-17 - action items (DO FIRST, before new features)
Decide now:
1. Single integration path. Embedding the crates is fine, but UI events must be the CLI's own stream-json lines verbatim, only adding `v` and `seq`. Drop the separate `DesktopEvent v1` names (e.g. `assistant_message` → `assistant`). No second schema.
2. Bump pinned rev `3a09220` → `5a1242d` (MCP output cap, tool results logged immediately; no API break). Note in CHANGELOG.
3. Fix red CI on 4e3cdea: `test engine (windows-latest)` - likely un-normalised Windows paths in snapshots.
Desktop fixes:
4. `AllowAll` is test-only; the app must use the CLI permission checker and ask the user through the UI.
5. No third log format: use the CLI session log and enable `journal` for resume.
6. Add a fallback arm when matching `AgentEvent` so new CLI variants don't break the build.
7. Hard-coded system prompt → use the CLI prompt builder.
8. RateGate and token budget belong in the CLI: open issues labelled `desktop` on atzenn/zencode instead of writing Desktop wrappers (replaces the earlier v0.3.0 RateGate/budget gate; v0.3.0 gate = PLAN-v0.3 milestone).
9. Create `docs/upstream-proposals.md`; proposals go out as issues (CLI repo is public).
10. Merge the three "Added" sections in CHANGELOG, remove frozen `core/` from CI, add `cargo audit`; consider moving `rebuild/` (cowsay) to its own repo (needs Zen - Z App cannot create repos).
CLI team (track only; open an issue if not yet filed): 11 `#[non_exhaustive]` on `AgentEvent`; 12 publish `docs/stream-json-v1.md` + NDJSON fixtures; 13 Bash tool on Windows; 14 release tags so Desktop pins a tag instead of a hash.
