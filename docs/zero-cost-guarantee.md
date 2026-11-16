# Zero-Cost Guarantee

**Zen Desktop cannot bill you. Ever.** This is not a pricing tier, a
promotion, or a setting you can misconfigure. It is structural.

## How the guarantee works

1. **No payment rails exist.** There is no checkout, no credit-card field, no
   billing API, and no place to store a paid-model API key. There is simply
   nothing to charge.
2. **The engine firewall rejects paid models.** `engine/src/cost_firewall.rs`
   validates every model id before any network call is made:
   - Allowed: ids ending in `:free` (the OpenAI-compatible zero-cost
     convention), or ids on an explicit allowlist of models verified to cost
     $0.00 per call.
   - Blocked: everything else - with a loud error, not a silent fallback:

   > Zen blocked model 'gpt-4': only :free models are allowed.
   > Zen cannot bill you - this is structural, not a setting.

3. **Three enforcement points.** The check runs at engine startup/config load,
   at per-task model selection, and across every model-chain fallback id - so
   a non-free model can never be reached, even by a fallback path.
4. **The spend meter proves it.** `SpendMeter` reports `$0.00` across every
   task, with a per-task cost preview that always reads "$0.00". The UI shows
   the number; the architecture guarantees it.

## What we will never do

- We will never add a "free tier" with paid billing behind it. (The community
  has learned to distrust that pattern - and rightly so.)
- We will never silently swap your `:free` model for a paid one.
- We will never send telemetry about your usage. See
  `docs/zero-telemetry.md` (R14) for the auditable network log.

## For developers

- `FreeOnlyPolicy::validate_all(primary, fallbacks)` - call this on any new
  model-selection path you add (e.g. the RateGate/ModelChain integration).
- `FreeOnlyPolicy::with_extra_allowlist(["my-lab/local-coder"])` - extends
  the policy with extra zero-cost model ids. Local and self-hosted models
  (Ollama, LM Studio, a relay) have no `:free` suffix, so the default policy
  blocks them; add their exact ids here after verifying they cost nothing.
  The engine's `run_task_with_sink` currently validates with
  `FreeOnlyPolicy::default()`, so the extra allowlist is not yet exposed
  through `RunOptions` or config - that exposure is an open follow-up.
- `FREE_ALLOWLIST` - add an id here only after a real API call returns
  `usage.cost == 0.0`. Documentation claims are not enough.
- Note on the guarantee's scope: the firewall validates the **model id
  string** client-side before any network call. It cannot prove what a
  remote gateway does with a `:free`-named route on its side of the wire -
  "cannot bill you" means the app never attempts a paid model, not that a
  dishonest gateway cannot mislabel one.
- `cargo test -p zen_engine cost_firewall` runs the firewall's test suite,
  including the proof that validation performs no I/O.
