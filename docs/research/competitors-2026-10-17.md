# ZenCode Desktop - Competitor research (2026-10-17)

> Scope: coding agent CLI/desktop, perspective of "running well on FREE models" (free models, free router; weak/slow models, frequent 429s, small context).
> Method: web_search (prioritize post-2026-08-01) + web_fetch original page when possible. Each point has a link.
> Convention: **[primary]** = original page opened (author's changelog/release/docs/blog); **[secondary]** = aggregated article/news, original page not opened; **unconfirmed** = no reliable source found.
> No repo modifications. No fabricated figures.

---

## Constraints that shape everything

| Constraint | Figure | Source |
|---|---|---|
| Free-endpoint rate limit | 20 req/min; **50 req/day** on the free plan | Free-endpoint docs |
| Failed requests still count | "Failed attempts count toward that daily cap" | Free-endpoint docs |
| 429 has headers | `X-RateLimit-Limit/Remaining/Reset`, sometimes `Retry-After`; exponential backoff recommended; "model fallbacks" supported | Free-endpoint docs |
| Quota check | `GET /api/v1/key` → `free_model_daily_requests {used, limit, remaining}` (resets 00:00 UTC) | Free-endpoint docs |
| 429 mid-stream | Arrives as SSE event `finish_reason: "error"` rather than HTTP 429 | Free-endpoint docs |
| Prompt caching on Free tier | Pricing table shows Prompt Caching = **No** on Free tier | Free-endpoint docs - *specific application per `:free` endpoint: unconfirmed* |

**Implication:** with 50 req/day, *each request* is a scarce resource → win by "more work per request" (small prompts, batched tool calls, fewer blind retries), not by prompt caching.
**(EN):** At 50 requests/day, request economy beats token economy; blind retries literally burn the daily budget.

---

## 1. Research

### 1.1 Competitor table

| Competitor | Language | Standout strength / Standout | Latest 2026 (date + link) | Weakness (with free user model) |
|---|---|---|---|---|
| **Claude Code** | TypeScript (npm) | Largest ecosystem: subagent (base, nested 3 levels), hooks (33 events), plan mode, skills, mods, `/goal`, Rewind "Summarize up to here" | 2.1.301–302 (15–16/10): dedicated model for auto-mode classifier, default subagent fork, hooks max 12/event [secondary] https://claude-for.com/guides/claude-code-2-1-301-302-2026-10-17 · 2.1.295 (08/10): `onFailure: "block"` for hook [secondary] https://dev.classmethod.jp/en/articles/20261009-cc-updates-v2-1-295/ · W20 (11–15/5): `/goal`, `continueOnBlock`, "Summarize up to here" [primary] https://code.claude.com/docs/en/whats-new/2026-w20 | Closed source, tied to a single vendor model; very large system prompt (estimated 5k–55k tokens depending on source - **unconfirmed** number) |
| **OpenAI Codex CLI** | **Rust** | Sandbox Linux bubblewrap+seccomp, macOS Seatbelt; apply_patch; Code Mode; parallel read-only tools | rust-v0.163.0 (15/10): steer between responses, run read-only tools in parallel, "Treat incomplete responses as terminal errors" (#53451), sandbox integrity checks [primary] https://github.com/openai/codex/releases/tag/rust-v0.163.0 · v0.162.0 (08/10): managed git worktrees [secondary] https://bisque.today/p/codex-cli/codex-cli-0-162-0 | Optimized for OpenAI model; custom provider must be compatible with Responses API; very large codebase (hundreds of PRs/release) |
| **OpenCode** (Anomaly, formerly sst) | TypeScript | 2.0: one background service for TUI/web/desktop; **Code Mode** (MCP/OpenAPI → JS functions, many tools/1 round-trip); background subagent; 80+ plugins; agent `plan` read-only | 2.0 (~12/10, Cloudflare changelog 12/10) [secondary, with launch note excerpt] https://automater.ai/news/opencode-2-0-one-background-service-code-mode-subagents-sdk/ · v2.0.30 (15/10) default diff panel [secondary] https://www.turboai.dev/blog/opencode-2-v2-0-30 · v1.18.17 retry count limit + jitter; v1.18.27 5-minute timeout for slow-starting model [secondary] https://www.turboai.dev/blog/opencode-v1-18-32 | Breaking changes 2.0 (1.x plugins broken, new API server); prompt ~15k tokens per one source - **unconfirmed** |
| **Crush** (Charm) | Go | Beautiful TUI, LSP context, MCP, multi-platform (including BSD/Android), Agent Skills | v0.98.0 (08/10): OSC 7501 Program Status, OAuth model outside catalog, MCP timeout [secondary] https://seismonews.com/event/charm-ships-crush-v0-98-0-with-grok-oauth-login · v0.98.1 (09/10) [primary] https://pkg.go.dev/github.com/charmbracelet/crush@v0.98.1/internal/agent | v0.x, no API commitment; FSL-1.1 license [secondary] https://kunavo.com/guides/crush-pricing |
| **Aider** | Python | **Repo map** (default `--map-tokens` 1k), multiple edit formats (whole/diff/udiff/architect), public edit benchmark | No major release in 2026: last tag v0.86.0 (08/2025), PyPI 0.86.2 (12/02/2026) [primary] https://github.com/Aider-AI/aider/tags · repo map docs [primary] https://aider.chat/docs/repomap.html | Development slowed; no full self-running shell agent loop (community fork cecli adds it [secondary] http://shortlisted.tools/products/aider) |
| **Gemini CLI** | TypeScript | Free tier; plan mode; skills; subagent routing | v0.64.0 (13/10): write files **atomically** (temp + rename), serialize same-path operations, state.json with backup/restore [primary date] https://geminicli.com/docs/changelogs/latest/ + [secondary details] https://aftergradient.com/news/2026/10/gemini-cli-0-64-0 | 10/2026 releases mainly bug fixes; optimized for Gemini |
| **Pi** (Earendil, Mario Zechner) | TypeScript (Node ≥22.19) | **Minimal**: 4 tools (read/write/edit/bash), system prompt + tool defs < 1,000 tokens; no MCP/subagent/plan mode by design; extensions/skills/packages | Pi 1.0 (01/10) + Pi Durable [primary] https://earendil.com/posts/pi-1-0/ · 1.1.0 (08/10) OSC 7501 [secondary] https://thegitreporter.com/articles/2026-10-09-daily-pi-and-crush-tell-the-terminal-when-they-need-you/ · 1.2.0 (12/10) [secondary] https://en.wikipedia.org/wiki/Pi_(AI_agent) | YOLO by default, no sandbox; requires Node; self-build advanced features |
| **Goose** (AAIF, originally Block) | **Rust** + TS | Agent loop as **state machine** split into `goose-agent` crate; 70+ MCP extensions; ACP; desktop+CLI+API | v1.54.0 (08/10): foreground subagent in state-machine loop, extension lease, vision detection [secondary] https://patchwiredaily.com/r/goose/1.54.0.html · v1.55.0 assets 16/10 [primary] https://github.com/aaif-goose/goose/releases · v1.47.0 (21/08) split agent loop into state machine [secondary] https://mindpattern.ai/f/21707 | General-purpose (not coding-specialized); heavy desktop package (~155 MB rpm) |
| **Cline** | TypeScript | IDE + CLI + SDK + Desktop (for open-weight model), checkpoint, plugin marketplace | CLI 3.0.54 (13/08): **reject truncated JSON tool-call instead of silently "fixing"**; token telemetry separate cache [primary] https://github.com/cline/cline/releases/tag/cli-v3.0.54 · Cline Desktop (3.0.62, 15/09) [secondary] https://freedom.tech/posts/2026-09-15-cline-cli-3-0-62/ | Heavy IDE; many daemons/hubs |
| **Kilo Code** | TypeScript (core synced from OpenCode) | 500+ models at original price, Ask/Plan/Code/Debug modes, Agent Manager, worktree | v7.8.10 (14/10, pre): Search Ignored Paths, fix grep encoding [secondary] https://terminalblog.com/blog/kilocode-v7-8-10-search-ignored-paths-grep-fix-mcp-windows/ · v7.8.0 sync OpenCode v1.18.19–32 [secondary] https://terminalblog.com/blog/kilo-code-v7-8-1-sbom-supply-chain-security/ | Very fast releases → minor bugs; owned by Anaconda since 15/07/2026 [secondary] https://andrew.ooo/answers/kilo-code-vs-cline-vs-continue-open-source-coding-agents-july-2026/ |
| **Zed agent** | **Rust** | Native agent in editor + **ACP** to run external agents (Claude Code, Codex, OpenCode, Cursor, Pi…); auto-compact thread | 1.23.2 (07/10): auto-retry on overload, ACP plan refresh [primary] https://zed.dev/releases/stable · v1.25.0-pre: compaction for external ACP agent [secondary] https://www.devagentradar.com/insights/radar-brief-2026-10-15 | Tied to Zed editor |
| **Amp** | unconfirmed | Multi-agent orchestration, orbs (remote machines), thread sharing | Free with BYOK (13/09) [primary] https://ampcode.com/news/free-agent · uses Claude Pro/Max (10/10) [primary] https://ampcode.com/news/use-your-claude-plan | "Amp Free" (ad-funded) discontinued [secondary] https://kunavo.com/guides/amp-pricing; targeting frontier models |
| **Cursor CLI** | unconfirmed | Headless/CI, ACP, sandbox, Auto-review | 2026.10.08: Auto-review asks before risky delete commands [secondary] https://www.vibehacker.com/news/cursor-cli-2026-10-08-makes-auto-review-ask-before-risky-deletes-and-flags-out-o · Projects (10/09) [primary] https://cursor.com/changelog/projects | Closed, requires Cursor account; ACP bug reports wrong model [primary] https://forum.cursor.com/t/cursor-cli-acp-reports-grok-4-6-fast-despite-explicitly-selecting-the-listed-non-fast-model/174835 |
**Cross-check external harness (dsh)** (from prior project research, not rechecked today): Cordis plugin, append-only session log, bwrap/Landlock/Seatbelt sandbox, token cost ~10x Pi.

### 1.2 Benchmark

- **Terminal-Bench**: harness affects scores. Same model, different harnesses differ by 2,8–8,1 points (e.g. Fable 5: 83,8% Claude Code vs 80,4% Terminus 2; GPT-5.5: 83,1% Codex vs 78,0% Terminus 2; confidence interval ±1,2–1,7% → differences < 3 points are unreliable) [secondary] https://note.com/hacklog_stealth/n/n2b39609cef45?hl=en , https://www.codewithseb.com/blog/terminal-bench-2-1-coding-agent-leaderboard-reality . Official leaderboard: https://www.tbench.ai/leaderboard/terminal-bench/2.0 (full numeric table not yet accessible - the above data is **not confirmed from the source**). Vals.ai has "archived" TB 2.0 due to saturation [secondary] https://www.vals.ai/benchmarks/terminal-bench-2.
- **SWE-bench Verified (bash-only)**: scores *all* models with mini-SWE-agent - ~100 lines of Python, bash only, **no tool-calling API** → works with any model; README states ">74%" [primary] https://mini-swe-agent.com/latest/ , https://www.swebench.com/verified.html . Prompt requires "exactly ONE bash block" per turn [primary] https://github.com/SWE-agent/mini-swe-agent/blob/60f566bd/src/minisweagent/config/benchmarks/swebench_backticks.yaml.
- **Lessons for ZenCode**: (a) a minimal harness is competitive; (b) need a separate eval suite running on free models because leaderboards do not measure weak models + 429.

### 1.3 Most valuable techniques / Top techniques (7)
1. **Minimal harness / Lean harness** - Pi: system prompt + 4 tool < 1.000 token [primary] https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ ; independent measurement: 550 token prompt + 588 token schema = 1.138 token/request [secondary] https://harrisonsec.com/blog/system-prompt-token-floor/ . mini-SWE-agent >74% SWE-bench Verified with only bash. *Why it works:* model has been RL-trained for the coding agent role; small prompt = less usage of small context, less "noise" for weak model. Pi also notes: MCP Playwright 21 tool = 13,7k token, Chrome DevTools MCP = 18k token occupies 7–9% context before work [primary, same article].
2. **Controlled lenient edit application / Flexible edit application** - Aider: disabling flexible patching → edit errors increase **9 times**; not using "high-level diff" → errors increase 30–50%; udiff raises GPT-4 Turbo from 20% to 61% on the 89-task refactor set; JSON/function-call is worse than text format due to escape [primary] https://aider.chat/2023/12/2
5. **Context management** - Claude Code Rewind "Summarize up to here" keeps the recent turns intact [primary] https://code.claude.com/docs/en/whats-new/2026-w20 ; Pi compaction (docs) + the suggestion "keep the latest ~20k tokens, summarize the older part" [secondary] https://stillbrook.dev/articles/conservative-to-the-token ; Pi `read` defaults to 2,000 lines + offset/limit [primary] Pi article; progressive disclosure (README/skill read only when needed instead of loading the full MCP schema) [primary] Pi article.
6. **Request economy** - Codex runs read-only tools in parallel (0.163) [primary]; OpenCode Code Mode: one script calls many tools in 1 round-trip, MCP goes through Code Mode by default [secondary, quoting the launch note]. At 50 req/day this is the biggest lever (fewer LLM turns per task).
7. **Safety & undo** - atomic file writes (temp+rename) and serialized writes to the same path (Gemini 0.64) [secondary detail]; hook `onFailure: "block"` (a failing hook blocks instead of passing) [secondary]; `continueOnBlock` returns the denial reason to the model and continues the turn [primary W20]; bwrap + seccomp sandbox (Codex docs) [primary 0.163]; checkpoint snapshots (Cline Desktop) [secondary].

> Prompt caching note: the free-endpoint docs describe sticky routing + caching at 0.1×–0.5× of the input price for supporting providers, but the Free plan lists "Prompt Caching: No". → For free users, **keeping a stable prefix is still worth doing (cheap, future-compatible)**, but expect no measurable benefit; low priority.

---

## 2. Ideas - 12 features for ZenCode (ranked by value for free-model users)

Effort: S ≤ 1 day · M 2–4 days · L ≥ 1 week. Milestones: **[v0.3.0] [v0.4.0] [later]**.
Architecture assumption: `core/` has the traits `Llm`, `Tool`, `Sandbox`, `SessionLog` (JSONL). HTTP/TLS choice is std-first (a minimal crate such as `ureq`+`rustls`, or calling `curl`) - the project's current decision is **unconfirmed**.

| # | Feature | Learned from | Why (free model) | Rust sketch in `core/` | Effort | Before/after metric | Milestone |
|---|---|---|---|---|---|---|---|
| 1 | **RateGate + ModelChain** (429-aware fallback chain) | Free-endpoint limits/fallbacks; OpenCode retry cap+jitter | 429s and the 50/day quota are the most common failures; blind retries burn quota | `core/llm/rate.rs`: `struct RateGate { rpm: TokenBucket, daily: DailyBudget }` reads `X-RateLimit-*`, `Retry-After`, `GET /api/v1/key`. `core/llm/chain.rs`: `struct ModelChain(Vec<Box<dyn Llm>>)` impl `Llm`; `enum LlmError { RateLimited{retry_after, scope: Minute\|Day}, ContextOverflow, Transient, Fatal }`; on `Day` → jump to the next model/router, no retry; on `Minute` → wait, at most N times + jitter; also handles mid-stream errors (`finish_reason:"error"`) | M | % of tasks completed on the 10-task eval with 1 free key; wasted (failed) requests per task; number of stops due to 429 | **v0.3.0** |
| 2 | **Prompt budget ≤ 1,200 tokens** (system + tool schemas) | Pi | Small context + every request is precious | `core/prompt/mod.rs`: static prompt, 3 tools (shell/read/str_replace) with short descriptions; `fn prompt_floor_tokens()` estimate (chars/4) printed in `--stats`; CI test fails above the threshold | S | fixed tokens/request (target ≤ 1,200; Pi measured 1,138) | **v0.3.0** |
| 3 | **Controlled lenient str_replace** | Aider flexible patching; Pi edit | Weak models often get whitespace/indent wrong → edit fails → extra requests | `core/tools/str_replace.rs` + `core/edit/matcher.rs`: matching ladder exact → normalize CRLF/trailing space → relative indent → line match ignoring blank lines; **exactly 1 match** required, on 0/≥2 return a structured error (top-3 similar regions + line numbers) | M | failed edits / total edits; "fix the edit" requests per task (Aider: flexible off → 9× errors) | **v0.3.0** |
| 4 | **Strict tool-call parser + text-mode fallback** | Cline 3.0.54; Codex #53451; mini-SWE-agent | Many free models lack or have unstable tool calling; truncated JSON | `core/parse/toolcall.rs`: validate with serde against the schema; truncated/missing JSON → `ToolError::Malformed` sent back to the model (never invented). `core/parse/text_fallback.rs`: if the provider has no tools support, require exactly 1 ```bash``` block or a `<<<str_replace>>>` block; capability stored in `ModelProfile` | M | % of turns with a valid tool call; number of free models that can run `zencode run` (target: including models without tools) | **v0.3.0** |
| 5 | **Truncate tool output + spill to file** | Pi read 2,000 lines; Gemini maxChars | One long `cargo build` can swallow a small context | `core/context/truncate.rs`: head+tail by token budget, the cut part is written to `.zencode/spill/<id>.txt` and its path reported so the model can `read` with an offset | S | average tokens per tool result; number of ContextOverflow events | **v0.3.0** |
| 6 | **Local eval suite + metrics from JSONL** | Terminal-Bench (harness gap), mini-SWE-agent | Without measurement we cannot tell which features help free models | `core/eval/`: 10–20 small tasks (sample repo + verify command); runner reads the SessionLog JSONL → requests/task, tokens/task, 429s, edit-fail, pass | S–M | baseline for every other feature | **v0.3.0** |
| 7 | **Multiple tool calls per turn + parallel read-only** | Codex 0.163; OpenCode Code Mode | Fewer LLM turns per task - the biggest lever at 50 req/day | `core/agent/loop.rs`: accept `Vec<ToolCall>` per response; `Tool::is_read_only()`; run read-only tools in parallel with `std::thread::scope`, write tools sequentially; prompt encourages batching | M | requests/task on the eval (target −25%, feasibility **unconfirmed**) | v0.4.0 |
| 8 | **"Keep recent, summarize old" compaction** | Claude Code Rewind summarize; Pi compaction | Free models have small contexts (often a few tens of k tokens - model dependent) | `core/context/compact.rs`: `trait Compactor`; above 70% of context: keep system + last N turns, summarize the older part with *the model in use* (1 request) or a mechanical summary that costs no request (files edited, commands run, last error); write a `compaction` event to JSONL | M | tasks lost to overflow; pass rate on long tasks | v0.4.0 |
| 9 | **Checkpoint/undo + atomic writes** | Gemini 0.64 atomic; Cline checkpoint; Claude Code rewind | Weak models often break files → cheap rollback needed | `core/fs/atomic.rs` (write temp + `std::fs::rename`); `core/checkpoint.rs`: snapshot files before each edit into `.zencode/ckpt/<turn>/`; `zencode undo [turn]` | S–M | time/ability to recover after a broken edit; 0 corrupt files when killed midway | v0.4.0 |
| 10 | **Verify loop `--until "<cmd>"`** | Claude Code `/goal` | Weak models often falsely claim "done"; checking with a real command costs no LLM request | `core/agent/goal.rs`: after each finished turn, run the verify command in the Sandbox; on failure → feed (truncated) stderr as a new turn; turn cap per DailyBudget | S | % false "done" on the eval; pass rate | v0.4.0 |
| 11 | **Plan mode = read-only tool set** | OpenCode `plan` agent; Pi `--tools read,...` | Cheap: plan with a fast free model, edit with a better one, or vice versa | `core/agent/profile.rs`: `AgentProfile { tools: ToolSet, model: ModelChainId }`; `zencode run --plan` writes `PLAN.md` | S | - (UX feature); measure requests in the plan phase | v0.4.0 |
| 12 | **Lightweight repo map (std-first)** | Aider repo map (1k tokens by default) | Fewer `ls/grep` turns exploring the codebase | `core/repomap.rs`: scan files honoring `.gitignore`, extract symbols with per-language regex/heuristics (fn/struct/class), rank by mentioned + recent files; 1k-token budget; tree-sitter later (heavy crate) | M–L | requests/task for "find & fix" tasks | later |
| + | Hooks (pre/post tool, `onFailure: block`, `continueOnBlock`) | Claude Code | Safety, automation | `core/hooks.rs`, argv exec without a shell | M | - | later |
| + | bwrap/Landlock/Seatbelt sandbox | Codex, dsh | Safer YOLO runs | `Sandbox` impl per OS | L | - | later |
| + | ACP server (runs inside Zed) | Zed ACP | Distribution: Zed lists Pi/OpenCode/Codex as external agents | `zencode acp` over stdio JSON-RPC | M–L | - | later |
| + | Pi-style subagents (calling `zencode run --print` itself) | Pi | No separate subagent mechanism needed | use the shell tool | S | - | later |

---

## 3. Results - Proposed roadmap v0.3.0 → v0.5.0

### v0.3.0 - *A real loop that survives a free key*
1. `zencode run "<task>"` with 3 tools shell/read/str_replace + SessionLog JSONL (planned milestone). **Done when:** all 10 eval tasks run in offline mode (mock `Llm`) and ≥ 1 real `:free` model completes ≥ 5/10.
2. RateGate + ModelChain (#1). **Done when:** simulated 429 tests (minute/day, with/without `Retry-After`, mid-stream error) all take the right branch; failed requests per task ≤ 1 on average on the eval.
3. Strict tool-call parser + text-mode fallback (#4) and lenient str_replace (#3). **Done when:** the fixture suite (≥ 30 whitespace/indent/CRLF/truncated-JSON cases) passes; edit-fail rate is logged; ≥ 1 model without tool calling works.
4. Prompt budget ≤ 1,200 tokens (#2) + output truncation (#5). **Done when:** a CI test asserts the floor ≤ 1,200 tokens (estimated); no tool result exceeds the configured budget.
5. Eval runner + metrics report (#6). **Done when:** `zencode eval` prints a table of requests/task, tokens/task, 429s, edit-fail, pass - the baseline for v0.4.0.

### v0.4.0 - *Fewer requests, harder to break*
1. Multiple tool calls per turn + parallel read-only (#7). **Done when:** requests/task on the eval drop versus the v0.3.0 baseline (report the real number, target −25%).
2. Compaction (#8). **Done when:** long tasks (≥ 30 turns) on a small-context model do not die from overflow; a compaction event appears in JSONL.
3. Checkpoint/undo + atomic writes (#9). **Done when:** a kill-midway test leaves no half-written file; `zencode undo` restores byte-exact content.
4. Verify loop `--until` (#10) + plan mode (#11). **Done when:** the false-"done" rate on the eval drops versus v0.3.0; `--plan` cannot call write tools (test).

### v0.5.0 - *Extensibility & safety*
1. Lightweight repo map (#12). **Done when:** requests/task for "find & fix" tasks drop versus v0.4.0; the map is ≤ 1k tokens.
2. Hooks (pre/post tool, onFailure block, continueOnBlock). **Done when:** a failing/timed-out hook blocks the action (test); the block reason is returned to the model.
3. Linux sandbox (bwrap or Landlock) → macOS Seatbelt. **Done when:** writes outside the workspace are blocked in an integration test on Linux CI.
4. ACP server for Zed (or skills/progressive disclosure if ACP is deferred). **Done when:** Zed accepts ZenCode as an external agent and runs 1 task (manual check + recorded).

---

## Report limitations
- Many release details come from secondary news sites (marked [secondary]); open the original changelog before quoting publicly.
- The Terminal-Bench "harness gap" figures have not been checked against the original tbench.ai board.
- System prompt sizes of Claude Code/OpenCode: sources conflict → unconfirmed.
- Implementation language of Amp and Cursor CLI: unconfirmed.
- The "−25% requests" target is a hypothesis to be verified by eval, not a measured figure.
