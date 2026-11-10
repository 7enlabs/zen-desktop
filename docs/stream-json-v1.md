# stream-json v1 - the `zen -p --output-format stream-json` contract

This is what the CLI emits **today** (zen 0.1.0). It is written from
`crates/zen-cli/src/output.rs` (`StreamJsonSink`, `result_json`) and
`crates/zen-cli/src/agent_run.rs`, and checked against real runs: the sample streams in
[`fixtures/stream-json/`](fixtures/stream-json/) are produced by the real binary against the mock
LLM, and `crates/zen-cli/tests/stream_json_contract.rs` fails when the binary's output no longer
matches them or when a record type, subtype or top-level field in them is missing from this page.
Regenerate after an intended change with
`ZEN_UPDATE_FIXTURES=1 cargo test -p zen-cli --test stream_json_contract`.

## Framing

- NDJSON on **stdout**: one JSON object per line, `\n`-terminated, UTF-8. Warnings and hook
  diagnostics go to **stderr** and are never part of the stream.
- Every record has a `type`: `system`, `assistant`, `user`, `stream_event`, or `result`.
  `system` records also have a `subtype`.
- Every record carries a top-level `v` (number): the stream-json schema version, currently `1`
  (issue #19). Bump it on any breaking change to the stream-json shape and note it in the
  CHANGELOG so downstream consumers (Desktop, other UIs) can gate on it. Additive-only
  changes (new records, new optional fields) do not bump `v`.
- **Flushing:** each record is written to stdout as one complete line and flushed the moment it
  is emitted (`Buffered::stdout` in `output.rs`). Consumers can read line by line and act on each
  record as it arrives (e.g. show `init`, `assistant`, `tool_progress` live). Lines from
  concurrent emitters (sub-agents, tool heartbeats) never interleave inside a line.
- Order: the `init` line first, then events in the order the agent loop reports them, then exactly
  one `result` line last (on a normal end, an error, or SIGINT). Exceptions: a run killed by
  SIGTERM exits 143 and prints nothing; a built-in slash command that is not available in `-p`
  mode prints only an error `result` line, with no `init`. Headless built-ins (`/cost`,
  `/context`, `/compact`, …) print `init` then `result`, with no events in between.
- No `seq` field exists in v1 stream-json records. (The session file written to disk uses
  `v`/`seq`; that is a different format.) A consumer that wants a sequence number numbers lines
  itself.
- Key order inside a record is not part of the contract (the fixtures are re-serialised with
  sorted keys). Unknown fields may be added in a minor release: ignore what you do not know.
- `uuid` is a fresh UUID v4 per record (not stable across runs, not referenced by other records).
  `session_id` is the run's session id (`--session-id` sets it) and is the same on every record.

## Records

### `system` / `init`

First line of every agent run.

| field | type | notes |
|---|---|---|
| `type` | `"system"` | |
| `subtype` | `"init"` | |
| `session_id` | string | |
| `cwd` | string | working directory of the run |
| `model` | string | effective model for this turn |
| `tools` | string[] | tool names offered to the model, in registry order (includes MCP tools `mcp__<server>__<tool>`) |
| `mcp_servers` | `{name, status}`[] | `status` is `"connected"` or `"failed"` |
| `permissionMode` | string | `default`, `acceptEdits`, `plan`, `bypassPermissions`, `dontAsk` |
| `slash_commands` | string[] | built-in and custom commands, with leading `/` |
| `folder_trusted` | bool | whether the working folder is trusted (add-only, since PR #5). In an untrusted folder project hooks, MCP servers, allow rules, `additionalDirectories`, env and verify are not loaded. `-p` runs are untrusted unless `--trust-folder` / `ZEN_TRUST_FOLDER=1` or the folder is in `~/.zen/trusted-folders.json`; The fixtures run with `ZEN_TRUST_FOLDER=1`, so they show `true` |
| `mcp_server_errors` | `{name, reason}`[] | **only present** when MCP config entries were skipped by validation |

The `init` line has **no** `uuid`.

### `assistant`

One per assistant message, including each continuation piece of a reply cut off by the output
token limit (they are separate `assistant` records; the stitched text is in `result.result`).

| field | type | notes |
|---|---|---|
| `type` | `"assistant"` | |
| `uuid` | string | |
| `session_id` | string | |
| `parent_tool_use_id` | string \| null | id of the `Task` tool call when the message comes from a sub-agent, else `null` |
| `message` | object | OpenAI-style chat message, see below |

`message` fields: `role` (`"assistant"`), `content` (string; omitted when the message only calls
tools), `tool_calls` (omitted when none): `[{id, type: "function", function: {name, arguments}}]`
where `arguments` is a **JSON-encoded string**. Null fields are omitted, not sent as `null`.

A tool call that was cut off by the token limit is never run and never appears in the stream.

### `user` (tool result)

One per tool result. The user's own prompt is not echoed.

| field | type | notes |
|---|---|---|
| `type` | `"user"` | |
| `uuid` | string | |
| `session_id` | string | |
| `parent_tool_use_id` | string \| null | `Task` call id for results inside a sub-agent, else `null` |
| `message` | object | `{role: "user", content: [{type: "tool_result", tool_use_id, content, is_error, parallel}]}` |

`content` is the text the model sees (string). `parallel` is `true` when the call actually
executed concurrently with another call of the same turn (multi-tool-call batching, capped by
settings `maxParallelToolCalls`, default 4); `false` for calls that ran alone. A permission
denial also produces a `user` record
with `is_error: true` for the same `tool_use_id`, after the `permission_denied` record. The
`Task` tool's own result (the sub-agent's final answer) has `parent_tool_use_id: null`; only the
sub-agent's inner messages carry the parent id.

### `system` / `api_retry`

Emitted before each retry of a failed model request (HTTP 429, 5xx, network error, or a 200 with
an error / empty body).

| field | type | notes |
|---|---|---|
| `type`, `subtype` | `"system"`, `"api_retry"` | |
| `uuid`, `session_id` | string | |
| `attempt` | number | 1-based retry number |
| `max_retries` | number | |
| `retry_delay_ms` | number | wait before this retry |
| `error_status` | number \| null | HTTP status, `null` for network errors |
| `error` | string | short error text (status and truncated body; never request headers) |

A provider error reported inside a `200` body after generation started (OpenRouter
`finish_reason: "error"` with an `error` object) is retried like the HTTP status it carries:
`error_status` is that code (e.g. `429`, `502`) and `error` starts with
`provider error mid-response: HTTP <code> (<error_type>)`. A mid-response 429 retries within
`retry.maxWaitSecs`; other mid-response errors retry at most 3 times, then the turn ends with an
`error_during_execution` result carrying the same text. A 402 is never retried, except
OpenRouter's in-flight budget 402 that sends `Retry-After`.

### `system` / `permission_denied`

| field | type | notes |
|---|---|---|
| `type`, `subtype` | `"system"`, `"permission_denied"` | |
| `uuid`, `session_id` | string | |
| `tool_name` | string | |
| `tool_use_id` | string | |
| `decision_reason` | string | currently always `"permission required in non-interactive mode or denied by rule"` |

### `system` / `hook_message`

A message meant for the user, not the model. Despite the name it is used for more than hooks.

| field | type | notes |
|---|---|---|
| `type`, `subtype` | `"system"`, `"hook_message"` | |
| `uuid`, `session_id` | string | |
| `hook_event_name` | string | a hook event name (`PreToolUse`, `Stop`, …) for a hook's `systemMessage`, or one of the CLI's own sources: `Truncation` (reply cut off and `maxContinuations` spent; answer may be incomplete), `Verify`, `PostCompact`, `SubagentStart` |
| `message` | string | |

### `system` / `compact_boundary`

Emitted when the conversation is compacted (see `fixtures/stream-json/compact_boundary.ndjson`).

| field | type | notes |
|---|---|---|
| `type`, `subtype` | `"system"`, `"compact_boundary"` | |
| `uuid`, `session_id` | string | |
| `trigger` | string | `"auto"` or `"manual"` |
| `pre_tokens` | number | prompt tokens before compaction |

### `system` / `prune`

Emitted when tool-result pruning replaces old results with extractive summaries
(not in the fixtures).

| field | type | notes |
|---|---|---|
| `type`, `subtype` | `"system"`, `"prune"` | |
| `uuid`, `session_id` | string | |
| `pruned` | number | tool results replaced with summaries |
| `tokens_saved` | number | input tokens no longer re-billed per turn |

### `stream_event`

Only with `--include-partial-messages` (not in the fixtures). Text deltas while a reply streams;
the complete message still follows as an `assistant` record.

```json
{"type":"stream_event","v":1,"uuid":"…","session_id":"…","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"…"}}}
```

**Consumer contract (issue #20):** `--include-partial-messages` is the required flag for
streaming UIs that render token-by-token output - without it, the stream contains only complete
messages. Deltas are **provisional**: the subsequent full `assistant` record is authoritative.
UIs must **replace** (not append) the accumulated deltas when the full message arrives, keyed by
`session_id` and turn order. Naive append-then-append-again double-renders the text.

### `system` / `tool_progress`

Emitted every 5 seconds while a single tool call is still running (issue #21). Lets a UI
distinguish "tool still working" from "CLI stalled/crashed" and show honest elapsed-time state.

| field | type | notes |
|---|---|---|
| `type`, `subtype` | `"system"`, `"tool_progress"` | |
| `v`, `uuid`, `session_id` | | as usual |
| `tool_use_id` | string | the running tool call's id (matches the later `user` tool_result) |
| `tool_name` | string | tool name, e.g. `"Bash"` |
| `elapsed_ms` | number | wall time since the tool call started |

No `tool_progress` is emitted for tool calls that finish in under 5 seconds. The `user`
record with the tool result always follows the last heartbeat for that `tool_use_id`.

### `result`

Always the last line.

| field | type | notes |
|---|---|---|
| `type` | `"result"` | |
| `subtype` | string | `success`, `error_max_turns`, `error_during_execution` |
| `is_error` | bool | |
| `duration_ms` | number | wall time of the turn |
| `duration_api_ms` | number | time spent in model requests |
| `num_turns` | number | agent-loop model calls in the turn (retries of one call are not counted) |
| `result` | string | final assistant text (continuations stitched), or the error text |
| `session_id` | string | |
| `total_cost_usd` | number | currently always `0.0` |
| `usage` | `{input_tokens, output_tokens, cache_read_input_tokens}` | summed over the turn; `cache_read_input_tokens` is the input served from the provider prompt cache (Anthropic `cache_control` / OpenAI automatic caching), 0 when unsupported |
| `model` | string | |
| `permission_denials` | `{tool_name, tool_use_id, tool_input}`[] | |
| `uuid` | string | |
| `stop_reason` | string \| null | derived from `subtype`: `success` → `"end_turn"`, `error_max_turns` → `"max_turns"`, otherwise `null` |

A reply stopped by the provider's content filter (`finish_reason: "content_filter"`) is not
requeried: any partial text is sent as an `assistant` record, then the turn ends with an
`error_during_execution` result whose `result` starts with `Blocked by the provider's content filter`.

Exit code: `0` when `is_error` is false, `1` when true, `130` after SIGINT (with an
`error_during_execution` result `"Interrupted by user"`), `143` after SIGTERM (no output).

## Fixtures

| file | shows |
|---|---|
| `plain_text.ndjson` | init → assistant text → result |
| `tool_call.ndjson` | assistant `tool_calls` (Read) → `user` tool_result → assistant → result |
| `subagent.ndjson` | `Task` call; the sub-agent's assistant message has `parent_tool_use_id: "call_task_1"` |
| `api_retry.ndjson` | HTTP 500 → `api_retry` → successful answer |
| `permission_denied.ndjson` | Write in default headless mode → `permission_denied` + error tool_result; `result.permission_denials` |
| `content_filter.ndjson` | `finish_reason: "content_filter"` with no text → one request, `error_during_execution` result |
| `mid_response_retry.ndjson` | 200 with `finish_reason: "error"` (429) → `api_retry` (`error_status: 429`) → successful answer |
| `mid_response_error.ndjson` | four 200s with `finish_reason: "error"` (502) → three `api_retry` → `error_during_execution` result naming `HTTP 502 (provider_unavailable)` |
| `truncation_note.ndjson` | two cut-off replies with `maxContinuations: 1` → two `assistant` pieces → `hook_message` `Truncation` note; `result.result` is the stitched text |

Normalised values in the fixtures: `uuid` → `"<uuid>"`, `duration_ms` / `duration_api_ms` → `0`,
the temp working directory → `"<cwd>"`, the temp home → `"<home>"`, the mock server address →
`"<mock>"`. The mock LLM reports 7 input / 3 output tokens per request.

## Compatibility

- Rust consumers of `zen_core::AgentEvent` (the enum behind these records) must match with a
  wildcard arm: it is `#[non_exhaustive]` since 0.1.0. A variant with no stream-json mapping
  emits nothing rather than an undocumented record.
- Any change to the records above updates this page and the fixtures in the same PR (the contract
  test enforces it) and is listed in `CHANGELOG.md`.
