# DV-5 first real run - 2026-11-19

> **Verbatim record** of the first real end-to-end run of Zen Desktop
> against the real Zen CLI binary. No credentials were involved at any
> point (anonymous free tier). Paths below are redacted per
> `docs/e2e/README.md` sanitization rules.
>
> **Verdict: NOT DONE.** The real agent run happened and the file was
> created by a real `Write` tool call, but the file holds 17 bytes
> (`hello zencode dv5`, no trailing newline) instead of the scoped 18
> bytes (`hello zencode dv5` + newline). Two attempts at the trailing
> newline failed; the step was stopped per the two-failure rule. See
> "What blocked" below.

## Setup

| Item | Value |
|---|---|
| Base | `main` @ `4e84bdb` (fetched live at run start; branch `dv5/first-real-run`) |
| CLI binary (`ZEN_BIN`) | built from `7enlabs/zencore` `main` @ `c87769a` (`cargo build --release -p zen-cli`, exit 0, 9m 29s) |
| Desktop binary | `zen-desktop` built from the same base (`cargo build --release --offline --bin zen-desktop` in `engine/`, exit 0) |
| Channel | Kilo anonymous free tier, `https://ai-gateway.kilo.ai/api/v1`, no API key |
| Model | default in code: `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free` (unchanged) |
| Task | Create `dv5-smoke.txt` in the working directory containing `hello zencode dv5` + newline |
| Sandbox | fresh temp dir, `max_turns = 12` |
| Cost | `$0.0` on both runs |

### Permission setup (part of the run, not a code change)

The first attempt with the stock setup failed before any file write:
in non-interactive (`-p`) mode the CLI denies `Write`/`Bash` instead of
prompting, and the engine passes no `--permission-mode` flag
(`dangerous_allow_all` defaults to `false` in `engine/src/lib.rs`).
Probe evidence: the model tried `Write` twice and `Bash` twice, all four
denied (`permission_denials`), file absent, exit 1, CLI hint:
"Re-run with --permission-mode bypassPermissions or add an allow rule."

The run therefore used the CLI's own sanctioned alternative, scoped to
the throwaway sandbox only:

- `<sandbox>/.zen/settings.json`: `{"permissions": {"allow": ["Write"]}}`
  (least privilege: only `Write` allowed, `Bash` stays denied)
- `ZEN_TRUST_FOLDER=1` in the environment (the CLI honors project
  settings only in a trusted folder; the env var is inherited by the
  child through the engine's spawn, which does not clear the environment)

No engine code was changed for this run.

## Run 1 - 08:09:01-08:09:09 UTC, 20 Nov 2026

Command (via the Desktop engine, `ZEN_BIN` set as above):

```text
zen-desktop run "Create a file named dv5-smoke.txt in the working directory
containing exactly this text on one line: hello zencode dv5"
--cwd <sandbox> --max-turns 12 --session-dir <sandbox>/.zen/sessions
```

| Metric | Value |
|---|---|
| Exit code | 0 |
| Turns | 2 |
| Input tokens | 11090 |
| Output tokens | 852 |
| `total_cost_usd` | 0.0 |
| `permission_denials` | 0 |
| Tool calls | 1x `Write` (`file_path: dv5-smoke.txt`, `content: hello zencode dv5`) |

Verification (read back, not the model's claim):

```text
$ od -c dv5-smoke.txt
0000000   h   e   l   l   o       z   e   n   c   o   d   e       d   v
0000020   5
0000021
bytes: 17
```

**Result: file created, content wrong.** 17 bytes, no trailing newline.
The model's result text claimed "without additional characters or
newlines" as if that were correct. Ground truth is the `od` output above.

## Run 2 - 08:09:37-08:10:06 UTC, 20 Nov 2026

Retry with an explicit newline instruction (file deleted first so the
agent creates it fresh):

```text
zen-desktop run "Create a file named dv5-smoke.txt in the working directory
containing exactly this text: hello zencode dv5 followed by a single trailing
newline (the file must be 18 bytes in total)"
```

| Metric | Value |
|---|---|
| Exit code | 1 |
| Turns | 3 |
| Input tokens | 17465 |
| Output tokens | 2764 |
| `total_cost_usd` | 0.0 |
| `permission_denials` | 0 |

**Result: `loop_stuck`.** The model issued the identical `Write` call
three times in a row (`content: hello zencode dv5`, still no `\n` in the
tool arguments) and the CLI's convergence guard stopped the run. The
model never attempted the newline in any tool call, so this is a
model-side failure to follow the instruction, not a tool stripping the
newline (unproven either way: no `\n` was ever sent). File still 17
bytes, verified by `od` again after the run.

Per the two-failure rule the trailing-newline step stops here; no third
prompt variation was tried.

## What blocked

- **Blocker:** `dv5-smoke.txt` is 17 bytes, not the scoped 18 bytes
  (`hello zencode dv5` + newline). Two attempts failed: run 1 wrote
  without a newline, run 2 repeated the identical no-newline `Write`
  three times and hit `loop_stuck`.
- **What is proven anyway:** the real Desktop -> real CLI (`ZEN_BIN`)
  path works end to end on the default free model: binary resolution,
  stream-json parsing, `Write` tool execution under the sandbox allow
  rule, session recording, `$0` cost.
- **Proposed next step (needs Leader decision):** one of (a) accept the
  17-byte file and close DV-5 with the content caveat, (b) allow `Bash`
  in the sandbox settings as well and run a follow-up step that appends
  the newline (`printf '\n' >> dv5-smoke.txt` via the agent), or
  (c) treat the missing-newline behavior as a CLI/model finding and
  open it as follow-up work instead of retrying prompts.

## Acceptance suites (engine, same base `4e84bdb`)

```text
cargo test --offline --locked --test contract_harness --test stream_json --test cli_spawn
contract_harness: 33 passed; 0 failed
stream_json:      30 passed; 0 failed
cli_spawn:         7 passed; 0 failed
Total:            70 passed; 0 failed
```

## Evidence kept

- Session NDJSON (Desktop `RecordingSink` output): run 1
  `session-1795162149.jsonl`, run 2 `session-1795162206.jsonl`
  (copies in the seat's goal bookkeeping, not in this repo).
- The 17-byte `dv5-smoke.txt` itself: sandbox
  `<sandbox>/dv5-smoke.txt` (copy in the seat's goal bookkeeping).
