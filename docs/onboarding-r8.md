# R8 - 60-Second Onboarding

**Goal:** a first-time user opens Zen Desktop and runs their first task
with a free model within 60 seconds.

## Flow

```
WELCOME → MODEL_SELECT → DEMO_TASK → DONE
              ↘ BYOK_SETUP ↗ (optional detour)
Any step → SKIPPED (user can bail out anytime)
```

1. **Welcome** - value prop ("the AI coding agent that cannot bill you"), 60s promise.
2. **Model select** - auto-probes free endpoints (`probeFreeModels`), pre-selects
   the fastest alive model. User can pick another or go BYOK.
3. **BYOK (optional)** - route dropdown + key reference. **Ban-risk warning**
   is shown before connecting (mirrors PR #66 knowledge):
   - Subscription OAuth login → **HIGH** (account bans reported since Feb 2026)
   - Consumer AI bundle route → **HIGH** (mass 403 bans reported)
   - Official key program route → **LOW** (official program)
   - The raw key is never stored - only an opaque key reference.
4. **Demo task** - one-click run of `"Explain in one sentence what Zen Desktop
   is."` against the chosen model.

## Files

- `desktop/src/onboarding.js` - state machine (pure logic), probe, persistence, UI
- `desktop/src/onboarding.test.js` - 19 unit tests (`node --test`)
- Wired in: `desktop/src/index.html` (script tag), `desktop/src/app.js` (boot),
  `desktop/src/styles.css` (overlay styles)

## Persistence

`localStorage["zen.onboarding.v1"] = { completed, skipped, modelId, at, v }`.
Onboarding shows only when no record exists. Corrupt JSON → treated as no record.

## Testing

```bash
node --test desktop/src/onboarding.test.js   # 19 tests, state machine only (no DOM)
node --check desktop/src/onboarding.js       # syntax
```

## Security notes

- No raw API keys in code, logs, or localStorage - only route label + key ref.
- Probe uses minimal `max_tokens: 1` ping; 8s timeout per model.
- `AbortController` ensures hung probes don't block the UI.
