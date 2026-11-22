# D8 - RateGate UI Design

**Date:** 2026-10-17
**Author:** ZD Leader
**Status:** Design only. The RateGate **policy** placement (CLI vs Desktop engine) is blocked on
[atzenn/zencode#10](https://github.com/atzenn/zencode/issues/10) - this document designs only the **UI
layer** and deliberately does NOT assume where the policy lives. The UI consumes a `QuotaState`
interface that either side can provide; the policy interface is marked **TBD pending #10**.

## Research

Quota/rate-limit UX is the market gap for a free-model-first desktop agent. Competitors
(`docs/research/competitors-2026-10-17.md`) confirm: free-endpoint models enforce a per-minute
and daily request cap, and *failed attempts count toward the daily cap* - blind
retry burns quota. OpenCode caps retries with jitter for exactly this reason.

How existing tools surface limits:

| Tool | What the user sees |
|------|-------------------|
| OpenAI dashboard | Usage bars per day, per-model spend, hard-limit email at 80%/100% |
| Claude (console) | Weekly usage meter with reset countdown, per-model breakdown |
| Free endpoint | `X-RateLimit-*` headers, `Retry-After`, key page shows remaining/day |
| Anonymous free tier | Hourly cap per IP, no UI - errors only |

What the UI needs from the policy layer (regardless of location): requests used vs limit (minute
and day scopes), reset timestamps, queue depth, active model + fallback reason. All of this is
captured in the `QuotaState` interface below.

## Ideas

### 1. Quota meter (always visible)

A compact meter in the status bar: `⚡ 32/50 today · resets in 4h12m`. Progress bar color shifts
green → amber (≥70%) → red (≥90%). Hover expands to per-scope detail (minute bucket + daily budget).
The meter reads `QuotaState` on every `DesktopEvent` and on a 30s poll fallback.

### 2. Offline queue

When a request would exceed the limit, the task is queued instead of failing: the UI shows
`⏳ Queued (3) - next retry in 42s`. Queue is FIFO with user-visible reorder/cancel. Auto-retry
fires when `QuotaState` signals capacity; the user can also force-retry (counts against quota -
shown as a warning). Queue survives app restart (persisted NDJSON, like sessions).

### 3. Auto-fallback indicator

When the policy switches models (e.g. daily cap hit → fallback model), the UI shows a banner:
`🔀 Switched to kimi-k2:free - deepseek-v4.1-flash hit daily cap (resets 00:00 UTC)`. The *reason*
is required - silent model switches erode trust. Dismissible, logged in the session transcript.

### 4. Warning states

| State | Trigger | UI |
|-------|---------|----|
| `healthy` | < 70% of any scope | Normal meter |
| `approaching` | ≥ 70% of minute or day | Amber meter + tooltip |
| `at_limit` | Scope exhausted | Red meter, queue mode on, banner with reset countdown |
| `cooldown` | `Retry-After` active | Countdown overlay on the input box, input disabled with explanation |

### Policy-location independence (TBD pending #10)

The UI never calls the policy directly. It subscribes to `QuotaState` updates pushed by whichever
side owns the policy. If the CLI owns it, the engine forwards CLI-emitted quota events; if the
Desktop owns it, the engine computes `QuotaState` itself. The interface is identical either way -
only the *producer* changes, which is exactly what #10 decides.

## Results

### `QuotaState` interface (JSON, v1)

```json
{
  "v": 1,
  "minute": { "used": 14, "limit": 20, "resets_at": "2026-10-17T09:58:00Z" },
  "daily": { "used": 32, "limit": 50, "resets_at": "2026-10-18T00:00:00Z" },
  "queue_depth": 3,
  "active_model": "deepseek-v4.1-flash:free",
  "fallback": {
    "active": true,
    "from": "deepseek-v4.1-flash:free",
    "to": "kimi-k2:free",
    "reason": "daily_cap_exhausted",
    "at": "2026-10-17T09:41:00Z"
  },
  "cooldown_until": null,
  "policy_owner": "tbd_pending_issue_10"
}
```

### UI component specs

1. **`QuotaMeter`** (status bar): renders `minute` + `daily` as dual progress bars; color by warning
   state; tooltip with exact numbers + reset countdowns; updates on `QuotaState` push or 30s poll.
2. **`QueuePanel`** (sidebar): lists queued tasks (id, title, enqueued_at, position); actions:
   reorder (drag), cancel, force-retry (with quota-cost warning); persists to
   `~/.zencode/queue.ndjson`.
3. **`FallbackBanner`** (top of chat): shows when `fallback.active`; content: from → to + reason
   + original model's reset time; dismiss button; auto-hides when fallback clears.
4. **`CooldownOverlay`** (input box): shown when `cooldown_until` set; disables input, shows
   countdown; re-enables automatically.

### Open questions for Z App

1. Should the queue be per-model or global? (Per-model preserves fallback semantics; global is
   simpler.)
2. Where does `QuotaState` polling live when the window is closed - does the tray service (D11)
   keep the meter fresh?
3. For `approaching` warnings: toast notification (D10) or status-bar-only? Toasts risk fatigue
   at 70% on a 20 req/min bucket.
