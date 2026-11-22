# DV-4: Contract-test harness pinning the CLI — Design doc (harness, 30/10/2026)

Final status (31/10/2026): tiers 1–3 are DONE and accepted — tier 3 was
accepted in writing by ZC Leader (message 1300, 30/10/2026). The branch
`dev/dv4-contract-harness` has been rebased onto main `5cc1090`. PR #7 is
OPEN, CI 13/13 green, awaiting Leader merge. This branch is NOT merged.
§1 below describes the tier-1 starting point (main `aeaf42d` had no parser
yet); it is kept as a record. PR #3 has since merged, so the tier-2 and
tier-3 items marked "waiting" in §§2–5 are now done — see the final-state
notes in §2, §5 and §7. The progress report due 19:30 02/11/2026 will
update this doc again after the PR #7 merge.

## 1. Key finding at the start (read before using the harness)

Main `7enlabs/zencode-desktop` at `aeaf42d1449909b168cf9cc168482161ec679de5`
(PR #2 merged) **did not yet contain the stream-json parser**: no
`engine/src/stream_json.rs`, no `engine/tests/stream_json.rs`, no
fixtures. The parser + tests + 21 fixtures lived on the DV-2 line
(`dev/fixtures-stream-json-b2-v2` = `aeaf42d` + 3 DV-2/B-2 commits), while
PR #3 was being rebased separately — that night's harness did **not** copy
parser code from there, did not check out that branch, did not touch
`/tmp/dv2/worktree`, to avoid duplicating PR #3 and conflicting when PR #3
merged.

Direct consequences, stated plainly:

- That night's tier 1 checked the contract at **fixture level (raw JSON)**,
  without calling the real parser — because main had no parser to call.
- The 21 fixtures were brought onto the DV-4 branch **byte-identical**
  (SHA256 verified against the DV-2 line), so when PR #3 merged, the
  identical fixture files merged cleanly, with no content conflict.
- The parser-bound asserts (tier 2, §4) were specified precisely down to
  each function name, each fixture, each expected value — after the merge
  they only needed adding, no re-research.

## 2. Where the harness lives, how it runs

| Component | Location on the DV-4 branch | Status |
|---|---|---|
| This design doc | `docs/dv4-contract-harness-design.md` | Done |
| Tier-1 test skeleton | `engine/tests/contract_harness.rs` (auto-discovered by cargo) | Done, green |
| Fixtures 01–21 | `engine/tests/fixtures/stream_json/*.jsonl` (byte-identical from the DV-2 line) | Done |
| New fixture 22 | `engine/tests/fixtures/stream_json/22_unknown_field_on_known_type.jsonl` | New, see §3 |
| Tier-2 tests (real parser) | In `engine/tests/contract_harness.rs` | Done — 13 tests, see §4 |
| Two-sided CLI pinning | Harness runs the real binary + parser (§5) | Done — first live run 30/10/2026, see §5 |

Run the harness:

```sh
cd engine
cargo test --offline --locked --test contract_harness
cargo test --offline --locked --test contract_harness -- --nocapture  # with the per-fixture report
```

Per-fixture report: the `fixture_inventory_report` test prints one line
per fixture (`fixture=<name> records=<n> types=[...]`; a rejected fixture
prints `rejected_input` with the reason: `unsupported_version`,
`missing_version`, or the count of bad JSON lines). The run log (full
`cargo test`) is attached with the acceptance report — never report
pass/fail counts from memory.

Immutable rules of the harness (written so later work does not break them):

- **Never edit the parser to force green.** If a tier-2 assert fails after
  the merge, keep the assert, report fixture + assert + actual behaviour
  as a finding, never silently drop the assert.
- **Additive files only** at the harness layer: design doc, tests,
  fixtures. Do not edit `engine/src/stream_json.rs`, do not commit
  `docs/stream-json-v1.md` (landing that contract doc awaits a separate
  Leader decision).
- New fixtures are added only when a hard case is found with no fixture
  yet; when adding, add files only, do not change the parser.

## 3. What tier 1 checks (20 tests, fixture level)

Real parser API (read from the DV-2 line, not yet on main at tier-1 time)
for reference: `parse_stream_json(&str) -> anyhow::Result<Vec<CliLine>>`,
`to_desktop_events(&CliLine) -> Vec<DesktopEvent>`,
`interpret_exit_code(i32) -> CliExit`,
`CliLine { v: u64, line_type: String, event: CliStreamEvent }`.
Tier 1 does **not** call these APIs; it checks the input conditions the
tier-2 asserts rely on, so tier 2 cannot go "green thanks to a wrong
fixture".

| HARD contract requirement | Tier-1 test | Fixture |
|---|---|---|
| Every record carries `v = 1` | `all_valid_fixtures_carry_schema_v1` | 01–09, 11, 12, 18–22 |
| NDJSON: each line is one object | `records()` in every test (panics with the physical line) | all |
| Blank lines dropped; blank lines do not change the record sequence | `blank_lines_yield_no_records` | 13 (empty), 14 (3 blank lines → 0 records) |
| Every record has a `type` | `every_valid_record_has_a_type` | all valid |
| No `seq` required | `no_valid_fixture_requires_seq` | all valid (no record has `seq`) |
| `system/init` carries `tools`, `model`, `session_id` | `system_init_carries_tools_and_model` | 01 |
| OpenAI tool-call shape: `message.tool_calls[]` with `function.arguments` a JSON-encoded string | `assistant_tool_calls_are_openai_shape` | 03 |
| Tool-only message omits `content` (does not send null) | `assistant_tool_calls_without_text_omit_content` | 11 |
| Multiple tool calls keep order | `assistant_multiple_tool_calls_keep_order` | 19 (read then bash) |
| `user` tool_result: `tool_use_id`, `is_error`, order preserved | `user_tool_results_shape_and_order` | 04, 05, 21 |
| `result` carries all inputs projected into `SessionEnd` (`subtype`, `is_error`, `num_turns`, `usage`, `duration_ms`, `model`) | `result_lines_carry_session_end_inputs` | 06, 07, 12, 20 |
| Unknown-type record is not dropped from the stream | `unknown_type_record_is_preserved_in_stream` | 09 (`heartbeat`, `at` field intact) |
| Unknown fields on known types are tolerated (additive) | `unknown_fields_on_known_types_are_tolerated` | **22 (new)** |
| `v` other than 1 is rejected | `bad_version_is_rejected_input` | 10 (`v = 2`) |
| Missing `v` is rejected | `missing_version_is_rejected_input` | 16 |
| Bad JSON: the physical line is identified | `malformed_json_fails_on_physical_line_1` | 15 (line 1) |
| Truncated stream: earlier lines valid, truncated line identified | `truncated_stream_fails_on_physical_line_3` | 17 (line 3 truncated, first 2 lines valid) |
| No trailing newline is still one complete record | `final_line_without_trailing_newline_is_a_complete_record` | 18 |
| Full session preserves order system → assistant → assistant → user → result | `full_session_order_is_preserved` | 08 |
| Per-fixture report | `fixture_inventory_report` | all |

The only new fixture that night — `22_unknown_field_on_known_type.jsonl`:
the hard case "unknown field tolerated" previously had **no fixture
covering it on a known record** (fixture 09 only had a strange field on a
strange type). Fixture 22 holds one `system/init` carrying `future_field`
+ `another_unknown`, and one `result` carrying `future_result_field`. At
tier 1, the test asserts the stream stays intact; at tier 2, the test
asserts the serde parser ignores those fields and still parses correctly
(serde structs ignore unknown fields by default).

## 4. Tier 2 — pinning the real parser (DONE — PR #3 merged, 13 tests in the harness)

When PR #3 merged, `engine/src/stream_json.rs` + `engine/src/lib.rs`
(`pub mod stream_json;`) appeared on main. The work was: rebase the DV-4
branch onto the new main, then add the following tests to
`engine/tests/contract_harness.rs` (without changing tier-1 tests or the
parser):

- `parse_stream_json` on each valid fixture returns the correct `CliLine`
  count, `v = 1`, `line_type` in the right order (notably 08: 5 lines).
- Fixture 09: `line.line_type == "heartbeat"` and
  `to_desktop_events(&line) == [DesktopEvent::Unknown { variant:
  "cli_stream:heartbeat" }]` — exactly 1 event, not dropped.
- Fixture 17: `parse_stream_json` returns `Err` (fail-fast: 0 events, no
  partial), message contains `line 3` and `invalid JSON`.
- Fixture 15: `Err` contains `line 1` and `invalid JSON`.
- Fixture 10/16: `Err` contains `unsupported schema version`.
- Fixture 14/13: `Ok` with an empty vec. The blank-line test with
  interspersed blank lines already existed on the DV-2 line
  (`blank_lines_are_ignored`); tier 2 keeps that test verbatim.
- Projecting `result` → `SessionEnd`: 06 gives `success = true`,
  `num_turns = 3`, `input_tokens = 1200`, `output_tokens = 340`,
  `duration_ms = 45210`, `model = ""` (absent model defaults to empty);
  07 gives `success = false`, result contains `429`; 12 gives
  `model = "deepseek-v4.1-flash"`; 20 gives `success = false` +
  correct model.
- Projecting `system/init` → `SystemMessage { event: "cli_system:init" }`
  containing `ses_abc123` and the model; `assistant` →
  `AssistantMessage` with text and `tool_calls` with correct id/name (03:
  `call_read_01`/`read`; 19: 2 calls in order; 11: `text = None`, 1 call
  `call_task_1`/`Task`); `user` → `ToolResult` with correct
  `tool_use_id`/`is_error`/order (21: `toolu_11` ok then `toolu_12` error).
- Exit codes: `interpret_exit_code(0) = Success`, `(1) = Error`,
  `(130) = Interrupted`, `(143) = Terminated`, anything else `Other(c)`.
- Tolerating additive change at the parser layer: fixture 22 parses `Ok`,
  first record is `System { subtype: "init", .. }`, last record is a
  successful `Result` — future fields do not change the outcome.
- No field loss in projection at the level the harness can check: every
  `CliLine` projects to at least 1 `DesktopEvent` (no line projects to 0
  events — including unknown types). This is the parser's "surface, never
  silently change" property.

Once tier 2 is green, the harness qualifies as a "parser-side harness" in
the true sense, and only then may it enter parser-layer acceptance.

## 5. Tier 3 — pinning the CLI on both sides (first live run DONE 30/10/2026, see docs/dv4-tier3-runbook.md)

This is the "pinning" part of DV-4 in the true sense: it needs both ends
present on the same main — the real CLI producing exactly the fixture,
and the Desktop parser reading exactly that fixture. First live run
completed 30/10/2026 (see the runbook); tier 3 accepted in writing by ZC
Leader (message 1300, 30/10/2026).

- **CLI side (pinning the real binary to produce exactly the fixture):**
  run the real CLI binary (`zen -p --output-format stream-json`) with a
  mock LLM, capture stdout, compare each line with the matching fixture
  in `engine/tests/fixtures/stream_json/` (inheriting the CLI-side
  mechanism `crates/zen-cli/tests/stream_json_contract.rs`: a fixture
  diverging from real output fails the test; a deliberate change means
  regenerate the fixture + update `docs/stream-json-v1.md` in the same
  PR). The Desktop harness at this tier does not generate new fixtures
  from a strange binary — the fixture is the standard, the binary must
  match the fixture, not the other way around.
- **Desktop side (pinning the parser):** tier 2 (§4) runs on exactly the
  fixtures the CLI side just proved to be real output. A CLI change that
  shifts a fixture fails on the CLI side first, never leaking down to the
  parser as a "new fixture silently matching a new parser".
- **The two-sided pinning point:** one fixture set + one contract
  `docs/stream-json-v1.md`. When the contract changes (v bump, new
  record, new mandatory field), both sides must change in the same PR:
  update the doc, regenerate the fixture on the CLI side, re-run tier 2
  on the Desktop side.

Sequence completed: (1) rebased the DV-4 branch onto the new main; (2)
tier-2 tests added (§4), green; (3) CLI-side harness connected (real
binary run, fixture comparison — see the runbook); (4) acceptance in
writing by ZC Leader received (message 1300, 30/10/2026). The remaining
step for the branch is the PR #7 merge, which belongs to the Leader, not
the seat.

## 6. 7 fidelity gaps — follow-up, NOT asserted in the harness

Per the B-3 cross-check, the following 7 gaps are field losses /
differences between the current parser and the full contract; they are
future check items, not asserts of this harness, and must not be used to
force green or force red:

1. `parent_tool_use_id` (assistant/user from a sub-agent) — the parser
   currently projects it to `None` on both sides.
2. `parallel` on `tool_result` — the parser does not keep this field.
3. `system` subtypes other than `init` (`api_retry`, `permission_denied`,
   `hook_message`, `compact_boundary`, `prune`, `tool_progress`) — the
   parser folds them into a text-form `SystemMessage`, without projecting
   to the Desktop's dedicated `ApiRetry`/`PermissionDenied`/
   `CompactBoundary` variants.
4. `result`: `duration_api_ms`, `permission_denials`, `stop_reason` —
   the parser does not read these fields.
5. `usage.cache_read_input_tokens` — `StreamUsage` has only input/output.
6. `uuid` / `session_id` on every record — the parser keeps `session_id`
   only on `system`/`result`, not `uuid`.
7. `stream_event` (delta with `--include-partial-messages`) — the parser
   has no dedicated variant; it currently falls into `Unknown` (not
   dropped, but not a true delta projection; the consumer must replace
   the delta with the full message).

When a gap is handled, add the matching fixture + test, record it in the
§3 table, and only then does it become a hard assert.

## 7. Final state (31/10/2026)

- [x] Branch `dev/dv4-contract-harness` rebased onto main `5cc1090`
      (tier-1 work started from `aeaf42d`).
- [x] This design doc (translated to English, intro finalized 31/10/2026).
- [x] Tier-1 skeleton green (20 tests, see §8 run evidence).
- [x] Fixture 22 for the hard case with no fixture.
- [x] Tier 2 (real parser) — PR #3 merged; 13 tests added; on the box:
      contract_harness 33 passed, stream_json 24 passed.
- [x] Tier 3 (two-sided CLI pinning) — first run 30/10/2026 done, see
      docs/dv4-tier3-runbook.md (binary ~/muse2/zen @ 290154c, localhost
      stub, parser check 1 passed). Tier-3 acceptance in writing by ZC
      Leader received (§5).
- [ ] PR #7 merge — the Leader's step, NOT the seat's. PR open, CI 13/13
      green. The seat does not self-merge.
- [ ] Separate decision on landing `docs/stream-json-v1.md` (untracked
      file, deliberately not committed in this harness).

Progress report due 19:30 02/11/2026 will update this table again after
the PR #7 merge.

## 8. Tier-1 run evidence (recorded at commit time; see the attached log when reporting)

Command: `cd engine && cargo test --offline --locked --test contract_harness`
(plus `-- --nocapture` for the per-fixture report). Detailed results —
command, exit code, pass/fail counts — live in the harness acceptance
report, not written from memory into this doc, to avoid drifting from the
real log.
