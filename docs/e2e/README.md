# docs/e2e - end-to-end run transcripts

Sanitized records of live runs against real `:free` model channels.
PLAN.md g2 requires proof of a real run; these transcripts are that proof,
scrubbed of anything sensitive.

## Sanitization rules

- **NEVER** include: API keys, bearer tokens, session tokens, cookies.
- **NEVER** include personal filesystem paths - use `<REDACTED>` (e.g.
  `<REDACTED>/.zen/sessions/...`).
- Allowed: model names, token counts, turn counts, durations, costs,
  task prompts, pass/fail outcomes.

## Naming

`YYYY-MM-DD-<slug>.md` - one file per run, dated in the runner's local day.

## Honesty

A transcript is either a verbatim record or explicitly labeled
**reconstruction**. Reconstructed files cite their sources and mark anything
unsourced `[unverified]` instead of inventing it.
