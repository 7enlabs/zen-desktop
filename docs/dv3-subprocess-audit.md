# DV-3 subprocess audit — stream-json v1 contract vs current Desktop

- **Revision tested:** `e0b54212d279c37f6678ea04e4bc02ecf1101326`
  (`org7enlabs/main`, PR #9 merge, 07:41 10/11/2026)
- **Branch:** `dev/rewire-subprocess` (created by this audit, from `e0b5421`)
- **Worktree:** detached at `e0b5421` on box `zencode-desktop`
  (`/data/home/dv3-audit`), main checkout untouched
- **Contract:** stream-json v1 — `"v": 1`, exit codes 0/1/130/143,
  CLI invoked via subprocess (`zen -p --output-format stream-json`)
- **Scope:** audit only. No production code changed in this turn.
  The rewire itself is NOT started — see Blockers.

## Method (zen-pstack)

LAUNCH: worktree detached at the exact revision under review (`e0b5421`,
`git log -1` printed before every check). DOCTOR: Linux box,
rustc/cargo 1.99.0, offline registry cache present. DRIVE: greps over
`engine/`, `core/`, `desktop/src-tauri/` plus real test runs.
EVIDENCE: every claim below cites a command + output. CLEANUP: worktree
kept for the rewire; no temp files left behind.

## Findings

### F1 — No production code spawns the CLI [CONFIRMED]

`grep -rn --include="*.rs" -E "Command::new|std::process"
engine core desktop/src-tauri` returns 16 hits; the only production
`Command::new` calls spawn `git` (see F4). There is no `zen` binary
invocation, no sidecar configuration, no `ZEN_CLI`/`ZEN_BIN` env
override, and no stdout line-reader for a child process anywhere in
`engine/src`, `core/src`, or `desktop/src-tauri/src`.

### F2 — The current run path is in-process, not subprocess [CONFIRMED]

`desktop/src-tauri/src/main.rs` (`run_task` command) builds
`RunOptions::new(base_url, model, cwd)` and spawns
`zencode_engine::run_task_with_sink(&opts, &prompt, sink)`.
`engine/src/lib.rs:221` builds `Agent::new(...)` from the `zen_agent` /
`zen-core` git dependencies with an `OpenAiClient` pointed at `base_url`.
The Desktop therefore runs the agent loop inside its own process today.
The rewire must replace this path (or add a subprocess path beside it) —
that is an architectural change, not a wiring fix.

### F3 — The parser and exit-code mapping conform to contract v1 [CONFIRMED]

`engine/src/stream_json.rs` implements `parse_stream_json`
(`"v": 1` enforced, unknown line types surface as
`DesktopEvent::Unknown`, never dropped) and `interpret_exit_code`
(0 Success / 1 Error / 130 Interrupted / 143 Terminated / other Other).
Real runs on `e0b5421`:

- `cargo test --offline --locked -p zencode_engine --test stream_json`
  → 24 passed, 0 failed, exit 0
- `cargo test --offline --locked -p zencode_engine --test contract_harness`
  → 33 passed, 0 failed, exit 0

Only test callers use `interpret_exit_code` / `CliExit`
(`engine/tests/contract_harness.rs`, `engine/tests/tier3_live_cli.rs`
— the latter is `#[ignore]`d and needs a `DV4_TIER3_CAPTURE` file).

### F4 — The only production subprocess use is `git` in checkpoints [CONFIRMED]

`engine/src/checkpoints.rs:177,193,209` — three `Command::new("git")`
calls (`.output()`, no timeout) via `head_exists()` / `git()` /
`git_bytes()`. This is outside the DV-3 CLI scope and is recorded here
only for completeness. The kickoff principle holds: the renderer never
calls subprocess directly.

### F5 — The authoritative contract doc is uncommitted [CONFIRMED]

`docs/stream-json-v1.md` (250 lines, written from
`crates/zen-cli/src/output.rs` and checked against real runs) exists
only as an **untracked** file in the main checkout
(`/data/home/zencode-desktop`); `ls docs/stream-json-v1.md` in the
`e0b5421` worktree fails ("No such file or directory"). The document
the parser cites as its authority is not in the repo at the tested
revision.

### F6 — No `dev/rewire-subprocess` branch and no 8-step plan [CONFIRMED]

- `git branch -a | grep -i rewire` → empty (local + all remotes).
  `git ls-remote --heads origin` and `--heads org7enlabs` → no rewire
  branch. This audit creates the branch.
- The "8-step rewire plan" said to be done 20/10 was **NOT FOUND**:
  greps for `rewire` / `8 bước` / `8-step` over `~/workspace` and over
  the box (`/data/home`) return only unrelated hits (RUSTSEC advisory
  texts). It is not re-created here — inventing it would violate the
  audit.
- Note: `origin/main` (`atzenn/zencode-desktop`, `d943148`) is a
  different line from `org7enlabs/main` (`e0b5421`). DV-3 bases on
  `org7enlabs/main`; `e0b5421` is confirmed an ancestor of it.

## Handoff (zen-pstack §5)

| Check | Verdict | Evidence |
| --- | --- | --- |
| `cargo fmt --check -p zencode_engine` | PASS | exit 0 on `e0b5421` |
| `cargo test --test stream_json` | PASS | 24 passed, 0 failed, exit 0 |
| `cargo test --test contract_harness` | PASS | 33 passed, 0 failed, exit 0 |
| `cargo clippy --workspace --all-targets` | NOT RUN | No code changed in this turn (audit only) |
| Test PC (Windows) | NOT RUN | Box is Linux only |
| CI | NOT RUN | No PR opened yet |
| Independent review | NOT RUN | Awaiting Leader review of this audit |
| Live CLI subprocess end-to-end | NOT RUN | No spawn code exists yet — that is the rewire |

## OBSERVABLE_SUCCESS for the rewire (defined now, executed after unblock)

Desktop runs a prompt through `zen -p --output-format stream-json` as a
subprocess; NDJSON lines stream through `parse_stream_json` into
`DesktopEvent`s live; exit codes map 0/1/130/143 per contract v1; the
`stream_json` (24) and `contract_harness` (33) tests stay green, and a
new live test with the real CLI binary passes. Out of scope: changing
the contract itself (`"v": 1`, exit codes, subprocess invocation).

## Blockers — rewire NOT started, needs Leader decision

- **B1 — 8-step rewire plan missing.** Said done 20/10, not found in
  workspace or on the box. Need: Leader locates it or re-issues it.
- **B2 — Rewire design decisions missing.** (a) How the Desktop
  resolves the `zen` binary (PATH lookup? sidecar? env override?);
  (b) how `RunOptions` (session, budget, scope, permissions, model)
  maps to CLI flags; (c) whether the in-process `zen_agent` path stays
  as fallback or is removed (dependency-removal scope); (d) stop/kill
  and timeout semantics for the child process.
- **B3 — Contract doc uncommitted.** Decide whether
  `docs/stream-json-v1.md` is committed with the rewire PR.

## Next step

Leader answers B1–B3; then the rewire proceeds on
`dev/rewire-subprocess` from this audit commit, 1 branch = 1 PR,
no self-merge.
