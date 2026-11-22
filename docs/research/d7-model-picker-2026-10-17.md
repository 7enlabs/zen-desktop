# D7 - Free-Only Model Picker

**Date:** 2026-10-17 · **Status:** Design (for Z App's g6 UI work)
**Constraint:** FREE-ONLY is a locked project decision. Paid usage needs Zen's approval.

---

## Research

### How the engine selects models today

On `origin/main`, model selection is a plain string with zero validation:

```rust
// engine/src/lib.rs
pub struct RunOptions {
    pub model: String,              // any string accepted
    pub fallback_models: Vec<String>, // also unvalidated
    ...
}
```

`run_task` / `run_task_with_sink` pass `opts.model` straight into `AgentConfig`
without checking anything.

### What "free" means

The `:free` suffix is the de-facto convention across free OpenAI-compatible
endpoints:

- `deepseek-v4.1-flash:free` - free tier
- `deepseek-v4.1-flash` - paid (same model!)

The suffix is the ONLY machine-readable marker. Names without it are treated
as paid unless explicitly allowlisted - fail closed.

### Why the picker must enforce, not just filter

A dropdown that merely hides paid models is insufficient: config files can be
hand-edited, the engine API is public, and future MCP/HTTP surfaces bypass UI
filtering entirely.

**Therefore: the engine must refuse non-free models at the `run_task` level.
UI filtering is UX; engine enforcement is the guarantee.**

---

## Ideas

### 1. Model registry

Bundled `models.json` (schema below) + refresh via `zencode models refresh`
(or Tauri command): re-fetch `/v1/models`, re-validate `:free`, update
`last_verified`. Models losing free status are flagged, never silently kept.

Fields: `id` (MUST end with `:free` unless allowlisted), `display_name`,
`provider`, `base_url`, `context_window` (shown in picker), `known_good`
(team smoke-tested), `last_verified` (free tiers change!).

**Explicit-free allowlist:** for models verified free without the suffix.
Each entry needs `verified_by` + `verified_at`. Empty by default.

### 2. Picker UI

- Dropdown with fuzzy search over `display_name` + `id`.
- Each row shows: display name, provider badge, context window
  (e.g., "128k"), and a green "verified" check when `known_good`.
- Unverified-but-free models appear in a separate "Untested" section.
- **Paid models never appear.** There is no "show paid" toggle - that would
  defeat the purpose. (If Zen ever approves paid use, that's a separate
  gated flow, not a picker option.)

### 3. Engine enforcement (the hard block)

New function in the engine (proposed location: `engine/src/model_policy.rs`):

```rust
/// Refuse any model that is not verifiably free.
/// Called at the top of `run_task` and `run_task_with_sink`.
pub fn ensure_free_model(model: &str) -> anyhow::Result<()> {
    if model.ends_with(":free") {
        return Ok(());
    }
    // explicit allowlist checked here
    anyhow::bail!(
        "model '{model}' is not a free model: refusing to run. \
         Free-only policy: use a ':free' model."
    )
}
```

Rules:
- Checked at the start of `run_task` / `run_task_with_sink`, before network I/O.
- Applies to `model` AND every `fallback_models` entry.
- Violation → hard error, no silent fallback to a default (that hides intent).

### 4. Layered defense

Picker UI lists only free models (UX) → config load warns on non-free ids →
engine `ensure_free_model` hard-blocks before any network call.

---

## Results

### Model registry schema (JSON, versioned)

```json
{
  "version": 1,
  "updated_at": "RFC 3339",
  "models": [{
    "id": "string, must end with ':free'",
    "display_name": "string",
    "provider": "string",
    "base_url": "https URL",
    "context_window": 0,
    "known_good": false,
    "last_verified": "RFC 3339"
  }],
  "explicit_free": [{
    "id": "string, verified free without suffix",
    "verified_by": "string",
    "verified_at": "RFC 3339"
  }]
}
```

### Enforcement rule spec

1. `ensure_free_model(model)` MUST be called at the start of `run_task`
   and `run_task_with_sink`, before any network I/O.
2. A model is free iff `id.ends_with(":free")` OR `id` is in `explicit_free`.
3. `fallback_models` entries are checked with the same rule.
4. Violation → `anyhow::Error`, no network call, message cites the free-only
   policy.
5. The registry file is data, not policy - the `:free` suffix check does not
   depend on the registry being present or fresh.

### Open questions for Z App

1. **Registry location:** bundle inside the Tauri app resources, or
   `~/.zencode/models.json` with a shipped default? (Impacts update flow.)
2. **Refresh cadence:** on every app start, daily, or manual only? (Free
   tiers change silently; stale data risks 402s mid-task.)
3. **Multi-provider same model:** if `x:free` exists on two providers with
   different rate limits, does the picker show both or pick the better one?
   (Suggest: show both, annotate limits - but needs a limits field.)
