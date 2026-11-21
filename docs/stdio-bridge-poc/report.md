# stdio NDJSON bridge PoC — report

- Assignment: ZC Leader, Telegram tin 13862, 16/11/2026, due 19:30 17/11/2026.
- Input: transport study `cli-desktop-transport-study-2026-11-16.md`, which
  recommends exactly one transport: stdio NDJSON stream-json v1 (the DV-3
  contract, already verified live with a real CLI binary).
- Branch: `dev/stdio-bridge-poc`, based on `origin/main`
  `bf7461b` ("fix: rename package/binary and repo references to zen-desktop
  (#16)").
- Scope: prototype only. It is input to decision D, not decision D itself.
  It does **not** decide the channel form (Agent Relay vs merging #18/#19);
  that waits for DV-5 and ZD Leader.

## Verdict

**Yes, it runs.** The Desktop engine spawns the real `zen` CLI binary as a
child process, speaks NDJSON stream-json v1 over stdio, parses every
envelope with the production parser, and measures per-envelope latency.
Five consecutive live runs all exited 0 with a complete
`system/init -> assistant -> result/success` stream, zero parse errors,
and empty stderr.

## What was built

| File | What it is |
| --- | --- |
| `engine/examples/stdio_bridge_poc.rs` | The PoC runner. Reuses the production pieces it measures: `cli_spawn::build_argv` builds exactly the argv `run_cli_task` would build (`zen -p --output-format stream-json --model … --base-url … --max-turns … --session-id … <prompt>`), and each stdout line is parsed with `stream_json::parse_stream_json` and projected with `to_desktop_events`. The only PoC-only part is the timing harness (arrival timestamp per envelope + JSON latency report). Includes 4 unit tests for the stats helpers. |
| `docs/stdio-bridge-poc/mock_llm_stub.py` | Localhost-only mock LLM (Python stdlib). Serves the exact canned OpenAI-compatible reply the CLI repo's own mock uses (`crates/zen-cli/tests/common/mod.rs`, `Reply::text`): "Hello from the mock LLM.", usage 7/3/10, model `mock-model`. Binds only `127.0.0.1:18123`, logs method + path + sequence only. |
| `docs/stdio-bridge-poc/run_poc.sh` | One-command reproduction: starts the stub, builds the example, runs it N times against a given binary, stops the stub. |
| `docs/stdio-bridge-poc/report.md` | This file. |

No new dependencies. No change to the production run path
(`engine/src/cli_spawn.rs` untouched).

## How it was run (reproduction)

CLI binary built from the CLI repo at
`f002d04b836c36fe4332fcf6cd8144e3a33e2305`
("fix(mcp): gate /proc-based teardown tests to Linux only (#207)"):

```sh
git clone --depth 1 https://github.com/7enlab/zencode.git <cli-checkout>
cd <cli-checkout> && cargo build -p zen-cli   # exit 0, 2m09s
./target/debug/zen --version                   # zen 0.1.0
# binary: target/debug/zen, 57,929,584 bytes
```

PoC run (final code, after `cargo fmt`):

```sh
./docs/stdio-bridge-poc/run_poc.sh <path-to>/target/debug/zen \
  f002d04b836c36fe4332fcf6cd8144e3a33e2305 5 <out.json>
# exit 0, prints "STDIO BRIDGE POC OK"
```

The child env per run mirrors the DV-4 tier-3 runbook (`docs/dv4-tier3-runbook.md`):
isolated `HOME` and cwd, `ZEN_API_KEY=poc-dummy-key` (dummy, never a real
key), `ZEN_TRUST_FOLDER=1`, `ZEN_TELEMETRY=0`, model `mock-model`,
`--base-url http://127.0.0.1:18123/v1`, prompt `hello`, `--max-turns 3`.
The stub never calls the outside network; the CLI called it exactly once
per run (stub log: 6 requests = 1 curl probe + 5 CLI calls).

## Latency results (canonical run, 5 runs, final code)

Round trip = spawn to child exit. Times are milliseconds since spawn.

| Metric | min | median | p95 | max | n |
| --- | --- | --- | --- | --- | --- |
| Wall time (spawn to exit) | 61.5 | 64.4 | 94.8 | 94.8 | 5 |
| Time to first envelope (`system/init`) | 40.7 | 44.8 | 75.8 | 75.8 | 5 |
| Time to `result` envelope | 58.5 | 62.5 | 92.5 | 92.5 | 5 |
| Inter-envelope gap | 0.2 | 8.5 | 20.8 | 20.8 | 10 |

Per-run detail: walls 94.8 / 64.4 / 64.4 / 70.6 / 61.5 ms; every run 3
envelopes (`system/init` 874 bytes, `assistant` 218 bytes,
`result/success` 408-410 bytes), each projecting to exactly 1 Desktop
event; exit 0; stderr empty; 0 parse errors.

Two earlier runs of the same 5-run recipe gave the same shape (medians
76.5 ms and 60.3 ms wall; the first run of the day showed a 311 ms
cold-start outlier on run 1). p95 here is nearest-rank on n=5, so it
equals the max — do not over-read it; the honest reading is "median
~64 ms, worst observed ~95 ms warm".

An independent direct capture (same binary, same stub, no PoC code in
the path) confirms the content is real, not fixture text:

```sh
zen -p --output-format stream-json --model mock-model \
  --base-url http://127.0.0.1:18123/v1 --max-turns 3 \
  --session-id 00000000-0000-4000-8000-000000000001 hello
# exit 0, stderr 0 bytes, 3 lines, all v=1:
# system/init -> assistant "Hello from the mock LLM."
#   -> result success, num_turns 1, usage input 7 / output 3, model mock-model
```

## Checks run (all on this branch unless noted)

| Check | Command | Result |
| --- | --- | --- |
| PoC unit tests | `cargo test --offline --locked --example stdio_bridge_poc` | 4 passed, 0 failed |
| PoC live run (final code) | `run_poc.sh … 5` | exit 0, `STDIO BRIDGE POC OK`, 5/5 runs clean |
| PoC failure path | example against `/bin/false` (exit 1, no stdout) | fails loudly, exit 1, "run 1 FAILED: exit Some(1) (Error) … no result line" — no silent pass |
| Direct capture | CLI binary + stub, no PoC code | exit 0, content verified (see above) |
| Full engine suite | `cargo test --offline --locked` | 179 unit passed; cli_spawn 7, contract_harness 33, stream_json 30, all passed, 0 failed |
| Tier-3 live test | `DV4_TIER3_CAPTURE=<capture> cargo test --offline --locked --test tier3_live_cli -- --ignored` | 1 passed |
| Format | `cargo fmt --check -p zencode_engine` | clean |

## Blockers and gaps (explicit)

1. **No Tauri wiring.** The PoC is an engine example binary. No Tauri
   command calls it, no frontend receives the events, and no sidecar
   packaging exists. The production run path (`run_cli_task`) already
   spawns the same way, but the bridge is not yet reachable from the UI.
   Needs a follow-up task (owner: ZD Leader / DV-5).
2. **No real-model latency.** All numbers use the mock LLM on localhost
   with a one-word prompt and a canned reply. Real-model round trips will
   be dominated by model time, not by the ~60 ms bridge overhead measured
   here. A real-model run needs a real key and is out of scope for this PoC.
3. **Trivial prompt only.** No tool calls, no multi-turn runs, no large
   outputs measured. The `assistant -> result` gap (~18 ms median here)
   will grow with real work; the bridge adds no per-envelope cost beyond
   line parsing (1 Desktop event per envelope in these runs).
4. **Stop/kill path not exercised by the PoC.** `cli_spawn` already
   implements the Leader-decided stop escalation (SIGINT, 5 s, SIGTERM,
   5 s, SIGKILL; tin 9078) and the existing `cli_spawn` tests cover it,
   but this PoC never sends a stop. Untested in this run, not broken.
5. **Binary resolution is still `ZEN_BIN` / PATH.** There is no sidecar
   yet; a user machine without `zen` on PATH gets the existing clear
   one-line error (`INSTALL_HINT`). Packaging the CLI with the Desktop is
   a separate decision.
6. **CLI telemetry notice.** The CLI prints an anonymous-usage notice to
   stderr by default. The PoC opts out with `ZEN_TELEMETRY=0` (the
   CLI's own documented opt-out) so stderr stays empty; without it, the
   notice appears but the stream is unaffected (stderr is never part of
   the stream by contract). The first PoC run caught exactly this and
   failed its own strict stderr check — fixed by the opt-out, not by
   loosening the check.
7. **Small sample.** n=5 runs, one machine, debug binaries on both ends.
   Treat the numbers as "bridge overhead is tens of milliseconds", not
   as a performance budget.

## What this does not decide

The channel form for decision D (Agent Relay vs merging #18/#19) is
explicitly out of scope and waits for DV-5 and ZD Leader. This PoC only
proves the transport the study recommended — stdio NDJSON stream-json
v1 — works end to end with a real binary and measures what it costs.
