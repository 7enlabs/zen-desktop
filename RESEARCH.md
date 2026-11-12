# ZenCode Desktop - Research / Ideas / Results (log)
## 2026-10-16
### Research
- Competitors: Neo-cowsay (Go): word-wrap, go-runewidth, cowfiles embedded in binary, --random, colors (--rainbow/--aurora), usable as library. charasay (Rust): textwrap + unicode-width, defaults to terminal width, --think, many colored charas. ferris-says (Rust): library API say(text,width,writer). gowsay/nmyk cowsay (Go): stdlib-first, full-width.
- Original npm cowsay@1.6.0: wrap is hard-cut by UTF-16 units, not by word; -l follows Node readdir order (filenames include ".cow").
### Ideas
- Keep default output byte-for-byte; new features only enabled via flags.
- Next: library say(text, opts), -W auto from terminal, optional ANSI colors, CI ubuntu/macos/windows + release binary.
### Results
- Rust std-only, 0 external crates. Golden parity 21/21, unit tests 5/5. --word-wrap done. 190 cowfiles embedded, -f/-r/-l/modes/-e/-T/--think done.
- cargo check OK for Windows (x86_64-pc-windows-gnu) and macOS (x86_64-apple-darwin); Linux release binary 506 KB.
### 18:40 addition
- Result: library API say(text, &Options) (learned from ferris-says/Neo-cowsay), -W auto from $COLUMNS (learned from charasay), CI GitHub Actions 3 OS (not pushed yet). Tests: unit 6/6, doctest 1/1, golden 21/21; Windows/macOS cargo check OK; binary 508 KB.
- Next ideas: measure real terminal width via ioctl/WinAPI (still std-only via FFI), optional ANSI colors (--rainbow like Neo-cowsay), push private repo to GitHub for real CI on macOS/Windows.

## 2026-10-16 - GitHub + Real CI
- **Research**: Windows checkout defaults `core.autocrlf=true` turning golden `.out` and cowfiles into CRLF → parity fail on Windows (21 cases). The GitHub App has contents/workflows/actions write on all repos but no administration (cannot create repo, cannot edit description/topics).
- **Ideas**: `.gitattributes` force LF for golden/cowfiles; later add release job (tag → build binary 3 OS → GitHub Release), commit `Cargo.lock`, clippy/fmt job.
- **Result**: private repo `7enlabs/zen-desktop`, 2 commits on `main`. CI run 38897073575: ubuntu ✅ macOS ✅ windows ✅ (first Windows fail due to CRLF, fixed). Artifacts binary for 3 OS. Added LICENSE, CHANGELOG, .editorconfig, Cargo metadata, CI runs only on push main/PR/dispatch, read-only permissions, concurrency.

## 2026-10-16 19:04 - Real Terminal Width + Release
- **Research**: `TIOCGWINSZ` differs by OS (Linux 0x5413, macOS/BSD 0x40087468); Windows gets `srWindow.Right-Left+1` from `GetConsoleScreenBufferInfo`. Done via std `extern "C"/"system"`, no `libc`/`windows-sys` crate needed.
- **Ideas**: Priority `$COLUMNS` → real terminal → 40 to keep golden tests stable; release on tag `v*` for Linux x86_64, macOS arm64, Windows x86_64.
- **Result**: commit cc20b2b, CI run 38897812171 green on Ubuntu/macOS/Windows. Release workflow ready, tag not pushed yet.

## 2026-10-17 - Lint Gate + ANSI Colour

### Research
- Neo-cowsay (Go) has `--rainbow`/`--aurora`/`--bold`; charasay (Rust) colors via external crate. Community standards: NO_COLOR (no-color.org: non-empty var = disable colors) and CLICOLOR_FORCE (bixense.com/clicolors: non-`0` = force colors).
- Windows 10+ only understands ANSI when `ENABLE_VIRTUAL_TERMINAL_PROCESSING` (0x0004) enabled via `SetConsoleMode`; legacy console refuses → drop colors.
- `std::io::IsTerminal` (std, Rust 1.70) replaces `atty`/`is-terminal` crates.
- clippy can run cross-target (`--target x86_64-pc-windows-msvc`) without a linker → catch `cfg(windows)` code errors directly on Ubuntu.

### Idea
- By default, color is disabled to maintain byte-for-byte parity; color is an extension with its own flag.
- Reset `ESC[0m` at the end of each line so color does not bleed; use `strip()` to test the paint→strip loop equals the original.
- Next: 256-color/truecolor (`COLORTERM=truecolor`), aurora gradient, bubble and cow coloring.

### Results
- CI adds a lint job: `cargo fmt --check` + `cargo clippy -D warnings` for Linux, Windows, macOS; build/test use `--locked`; commit `Cargo.lock`.
- Module `color.rs` (std-only, with Windows FFI), flags `--cow-color`, `--color=WHEN`; 2 unit tests + 1 new integration test; golden 21/21 unchanged (default no color when piped).

## 2026-10-17 - Competitor: DeepSeek Harness (dsh)

### Research
- dsh: MIT open-source agent harness from DeepSeek (released 13/08/2026, developer preview, source version 0.1.5-alpha.1), TypeScript monorepo on Cordis (plugin framework). Run `npx @deepseek-ai/dsh web` (Web UI 127.0.0.1:3080), has Desktop app, `headless` profile for one-shot tasks, Python SDK via JSON-RPC over stdio.
- "Everything is a plugin": model adapter (`ctx.llm`), tool registry, sandbox, session log, UI and also agent loop are all plugins; loop emits public events (`turn/start`, `system-prompt/assemble`, `tools/pre-execute`, `tools/post-execute`...).
- Cordis: effects can be reversed (LIFO, removing plugin returns system to previous state) + coeffect reactivity (declared dependencies, auto-reactivate).
- Session log append-only, invariant "model-visible means logged" → resume/fork/replay/Trajectory view.
- 4 modes: Standard, Code (`run_code`: model writes TS calling many tools at once), Minimal (shell + str_replace_editor, used for benchmark), Creator.
- Sandbox: Linux bwrap + Landlock (C launcher fail-closed), macOS Seatbelt, Windows restricted ACL token; self-reports "partial" on old kernel.
- Subagent can be Claude Code / Codex; MCP, skills, goals/jobs, schedule, webhook, LSP.
- Weaknesses (ArceApps/Justin3Go): token consumption ~10x Pi on same model (4.5K vs 47.6K), bug injecting CLAUDE.md + AGENTS.md twice, plugin runs in-process (no sandbox), plugin ecosystem not stable (41 compatible / 219 need review), API changes frequently.

### Idea for ZenCode Desktop
- "seam" architecture: split traits `Llm`, `Tool`, `Sandbox`, `SessionLog` in Rust; plugin = crate/feature or subprocess via JSON-RPC stdio (safer than dsh because plugin out-of-process).
- Session log append-only JSONL + replay/fork; invariant "everything the model sees is logged".
- Cross-platform std-first sandbox: Landlock syscalls (Linux), sandbox-exec profile (macOS), Job Object/restricted token (Windows) - fail-closed, honestly report partial.
- Minimal mode (shell + str_replace) serves as baseline for evaluating free model; avoid duplicating CLAUDE.md/AGENTS.md (hash content, dedupe).
- Token saving is a competitive advantage because we only use the free model.

### Results
- Not yet installed dsh (requires Node + DeepSeek API key; DeepSeek API is paid → not used). Could try later with a free OpenAI-compatible endpoint via relay.

## 2026-10-17 - Aurora gradient (truecolor / 256 colors)

### Research
- Neo-cowsay `--aurora`: color gradient along diagonal per character. Common truecolor detection standard: `COLORTERM=truecolor|24bit` (termstandard/colors); if absent, use xterm 256-color table (6x6x6 cube, codes 16–231).

### Idea
- Each non-whitespace character is colored by hue `(row + col/2) * 12°`; emit SGR codes only when the color changes to keep output compact; reset at the end of each line.
- 256-color fallback by rounding RGB to the cube, keeping std-only.

### Result
- `--cow-color aurora` + `Depth` (`COLORTERM`), API `paint_depth`; 2 unit tests + integration test (truecolor and 256 colors, strip = original). fmt/clippy clean, all tests pass, golden unchanged.

## 2026-10-17 - Agent core: crate `core/` (seam traits)

### Research
- From dsh: everything (model, tool, sandbox, session log, loop) is a plugin connected through a seam; the session log is append-only with the invariant "model-visible means logged", enabling resume/fork/replay. Pi keeps the loop minimal, using few tokens.

### Idea
- Four Rust traits instead of in-process plugins: `Llm`, `Tool`, `Sandbox`, `SessionLog`; the `run_turn` loop depends only on traits → swapping backends (fake in tests, free OpenAI-compatible channel later) does not touch the loop.
- The log invariant is kept "by construction": a single `see()` function both logs and pushes the message into context; context is always initialized from `replay_context()`.
- Sandbox is fail-closed by default (`NoSandbox` rejects everything and reports `Isolation::None`), never overstating the isolation level.

### Result
- Crate `zencode_core` 0.1.0 std-only, 6 integration tests pass; fmt/clippy `-D warnings` clean; `rebuild/` unchanged (golden parity pass). CI runs the `rebuild` + `core` crate matrix.
- Next step: `JsonlLog` (append-only JSONL on disk, std-only JSON encode/decode) + hash-based dedupe of AGENTS.md/CLAUDE.md.

## 2026-10-17 - Research on 13 competitors (details: docs/research/competitors-2026-10-17.md)

### Research
- Claude Code, Codex CLI, OpenCode, Crush, Aider, Gemini CLI, Pi, Goose, Cline/Kilo, Zed, Amp, Cursor CLI + Terminal-Bench/SWE-bench.
- OpenRouter :free: is limited to 50 requests/day, failed requests also count; the Free plan has no prompt caching.
- Aider: disabling flexible matching → edit errors increase ~9x; mini-SWE-agent using only bash still >74% SWE-bench Verified; Pi's system prompt ~1.138 tokens.

### Idea
- 12 features ranked by value for free-model users; prioritize RateGate/ModelChain, strict parser + text-mode fallback, lenient str_replace, prompt budget ≤1.200 tokens, multiple tool calls/turn.

### Result
- The v0.3.0 → v0.5.0 roadmap has been added to PLAN.md; regular development sessions follow this order.

## 2026-10-17 (session 12:05)
- Research: none.
- Idea: JSONL log self-repairs a corrupted tail to resume after crash/429.
- Result: step (b) done; core 10 tests + rebuild green locally.

## 2026-10-17 12:40
- Research: zen-agent already has OpenAiClient + Agent::run_turn; just wire it up.
- Idea: g2 adds the `zencode run` binary on the engine.
- Result: g1 done, 4/4 tests green locally.

## 2026-10-17 (session 13:00)
- Research: the CLI's AgentEvent has 7 variants; PermissionDenied carries tool_input (may contain secrets).
- Idea: the Tauri bridge emits Envelope NDJSON directly to the UI.
- Result: schema v1 + snapshot; remove tool_input from Desktop events.
