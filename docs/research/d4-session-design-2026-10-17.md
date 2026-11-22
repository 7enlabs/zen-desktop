# D4 - Session list / search / rename / fork (design)

Design for the Desktop UI session manager. Docs-only; no implementation.

## Research

**What the engine persists today (verified on `origin/main`):** nothing to disk.
`engine/src/events.rs` provides `RecordingSink` (in-memory `Vec<Envelope>`), `to_ndjson()`
(one JSON object per line, `{"v":1,"seq":N,...event}`), and `parse_ndjson()` (rejects lines
with `v > SCHEMA_VERSION` instead of misreading them). No code on main writes session files;
session NDJSON under `.zencode/sessions/` exists only in unmerged PR #4's `zencode` binary.

**What the UI needs (D4):** list past sessions, search them, rename, fork from any point.
The event schema (`DesktopEvent`, v1) is already stable and versioned, so sessions are just
append-only NDJSON logs plus metadata.

**Claude Code reference:** the `claude-code-learnings` doc is not on main (unmerged PR #4),
so no verified claims are made here. The general pattern (JSONL transcript per session +
metadata for listing) is what this design follows.

## Ideas

**Storage layout** (new; nothing on main contradicts it):

```
~/.zencode/sessions/
  <session-id>.ndjson      # append-only Envelope lines, written as events arrive
  index.json               # metadata for fast list/search; rebuilt from scans if lost
```

Session id: `YYYY-MM-DD-<12 hex chars>` (sortable, unique enough, human-greppable).

**Index schema** (`index.json`):

```json
{
  "version": 1,
  "sessions": [
    {
      "id": "2026-10-17-9f3a2c1b4d5e",
      "title": "Fix login redirect bug",
      "preview": "Fix the login redirect loop on /dashboard…",
      "created_at": "2026-10-17T09:00:00Z",
      "updated_at": "2026-10-17T09:15:32Z",
      "message_count": 12,
      "model": "deepseek-v4.1-flash",
      "cwd": "/home/user/myproject",
      "parent_id": null
    }
  ]
}
```

`preview` = first user message truncated to 120 chars (enables search without opening files).
`parent_id` = set only on fork.

**Search:** substring (case-insensitive) over `title` + `preview` for v1. This is O(n) over a
small index - fine for hundreds of sessions. Reach for tantivy only if sessions exceed ~10k
or full-transcript search is required; not before.

**Title:** default = first user message truncated to 60 chars. Optional LLM-generated title
later; never blocks session creation.

**Write path:** append each `Envelope` to `<id>.ndjson` as it is emitted; update `index.json`
(updated_at, message_count) on session end or every N events. `index.json` is a cache - if
corrupt/missing, rebuild by scanning `*.ndjson` headers (first user message + file mtime).

## Results

**Operation specs:**

- **list(limit, offset):** read `index.json`; sort `updated_at` desc; return page. Must not
  open any `.ndjson`. Empty index → empty list, never an error.
- **search(query):** case-insensitive substring over `title` and `preview`; rank title hits
  above preview hits, then `updated_at` desc. Empty query == list.
- **rename(id, new_title):** update `title` in `index.json` only; touch `updated_at`.
  Must not rewrite the `.ndjson`. Unknown id → error, no partial write.
- **fork(id, at_seq?):** copy `<id>.ndjson` → `<new-id>.ndjson`, truncating after `at_seq`
  if given (fork from a point); set `parent_id: <id>` in index; title = old title + " (fork)".
  The original is never modified. Forking a fork chains `parent_id`.

**Invariants:** `.ndjson` files are append-only (except fork-truncate at creation); `index.json`
is always rebuildable; every index entry has a matching `.ndjson` (orphans pruned on rebuild).

**Open questions for Z App:**
1. Session scope: global `~/.zencode/sessions/` only, or also per-project `.zencode/sessions/`
   in cwd (PR #4 used the latter)? Which is canonical for the UI?
2. Fork semantics: copy full history (simple, chosen above) or start a fresh log that
   references the parent's messages (saves disk, complicates resume)?
3. Should `search` eventually scan full transcripts, or are title+preview enough for v1?
