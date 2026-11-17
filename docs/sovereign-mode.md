# Sovereign Mode (R5)

> When Sovereign Mode is ON, your code **never leaves your machine**.
> All LLM inference goes to a local Ollama instance; every other network
> egress the engine initiates is blocked.

## For the impatient

```bash
ollama serve            # start local Ollama (once)
ollama pull qwen2.5-coder:14b   # fetch a coding model (once, then offline)
# In Zen Desktop: toggle "Sovereign Mode" ON.
# Disabling downloads the session attestation (compliance receipt).
```

## What it guarantees

| Guarantee | How |
|---|---|
| LLM inference stays on-device | Engine pins `base_url` to `http://localhost:11434/v1`; any remote endpoint is a hard error, never silently rewritten |
| No API keys in play | Any configured key makes the config non-sovereign; the mode refuses remote endpoints outright |
| No surprise egress | Every engine-initiated URL is checked against the loopback-only policy *before* any socket opens; blocked attempts are logged as evidence |
| Provable after the fact | A SHA-256 hash-chained attestation log (`hash-chain-v1`) records genesis, config validations, and every blocked egress; verifiable offline |

## Threat model

**In scope** (what Sovereign Mode defends against):

- Accidental misconfiguration sending prompts to a cloud endpoint.
- A compromised or curious model gateway logging your code - there is no
  gateway; inference is local.
- Telemetry exfiltration by the agent harness itself - combined with the
  zero-telemetry network log (R14), every outbound request is both
  policy-checked and audited.

**Out of scope** (honest limits - do not promise these):

- The OS, browser, or other applications exfiltrating data. Sovereign Mode
  is enforced at the *application layer* of this engine. Full assurance
  needs OS firewall rules too.
- A tampered Ollama binary. Verify your Ollama install separately
  (release signatures, package hashes). Sovereign Mode probes that *something*
  listens on `127.0.0.1:11434`; it does not attest the binary.
- Shell tool escapes: `BashTool` running `curl https://…` executes outside
  this policy. Confining tool subprocesses is the sandbox module's job.
- DNS lookups for blocked hosts may still leave the machine (DNS is not
  HTTP). Use a local resolver for full assurance.
- The attestation proves **the engine's** behavior, not the whole system's.
  The chain is hash-linked, not asymmetrically signed (minisign signatures
  are tracked follow-up work).

## GDPR mapping (EU)

Sovereign Mode is designed for the strictest reading of the GDPR:

| GDPR concept | How Sovereign Mode addresses it |
|---|---|
| Art. 5(1)(c) data minimisation | No personal data leaves the device at all - minimisation by architecture, not policy |
| Art. 25 data protection by design | Loopback-only egress is structural: the code path for remote inference does not exist while the mode is on |
| Art. 28 processors | There is no processor and no sub-processor, because there is no transfer |
| Art. 32 security of processing | Local-only processing + attestation log of blocked egress attempts |
| Art. 44–50 transfers to third countries | No transfer occurs; the transfer chapter is vacuous |
| Art. 30 records of processing | The attestation document (`sovereign-attestation-<session>.json`) is a ready-made processing record |

> **Note:** this mapping is an engineering analysis, not legal advice. Your
> DPO should review it against your specific processing activities.

## Enabling / disabling

1. Start Ollama: `ollama serve` (the mode probes `127.0.0.1:11434` and
   refuses to engage if nothing listens - it will not promise local
   inference it cannot deliver).
2. In the desktop UI, toggle **Sovereign Mode** ON.
3. The preflight check runs first:
   - **Remote endpoint configured** → must be fixed (change to Ollama
     localhost); cannot be acknowledged away.
   - **API key present** → warning; the key is never used while the mode
     is on, but you must tick "I understand" (a key implies a remote
     account you might have expected to be used).
   - **Ollama unreachable** → must be fixed; start Ollama.
4. Disabling the mode downloads the session attestation JSON - keep it as
   your compliance receipt.

## Verifying an attestation offline

```bash
# The attestation is JSON: entries[] each carry seq, ts, kind, detail,
# prev_hash, hash. Recompute with any SHA-256 tool:
#   hash_i = SHA256("{seq}|{ts}|{kind}|{detail}|{prev_hash}")
# and check hash_0.prev_hash == "GENESIS", final_hash == last hash.
python3 - <<'EOF'
import json, hashlib, sys
att = json.load(open("sovereign-attestation-<session>.json"))
prev = "GENESIS"
for i, e in enumerate(att["entries"]):
    assert e["seq"] == i and e["prev_hash"] == prev, "chain broken"
    h = hashlib.sha256(f"{e['seq']}|{e['ts']}|{e['kind']}|{e['detail']}|{e['prev_hash']}".encode()).hexdigest()
    assert h == e["hash"], f"hash mismatch at seq {i}"
    prev = e["hash"]
assert prev == att["final_hash"], "final hash mismatch"
print("attestation VALID:", att["policy_version"], att["scheme"])
EOF
```

The engine exposes the same check as `sovereign::verify_attestation`.

## Integration notes (for merge)

- The zero-telemetry choke point (`netlog.rs::logged_fetch`, PR #59) must
  call `SovereignMode::check_egress(&url)` **before** opening any socket
  when a mode is engaged. See the doc comment at the top of
  `engine/src/sovereign.rs` for the exact insertion.
- The model picker must hide non-Ollama providers while the mode is on;
  the UI badge (`desktop/src/sovereign.js`) already reflects the state.
- Tauri commands to implement: `sovereign_status`, `sovereign_enable`,
  `sovereign_disable` (returns attestation), `sovereign_preflight`.
