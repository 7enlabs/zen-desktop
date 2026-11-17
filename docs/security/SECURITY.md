# SECURITY.md - Open Security Decisions (R7 sweep, 2026-10-17)

These items were found by the adversarial R7 security sweep but need **Z App's
design decision** (or a follow-up PR) rather than a unilateral fix. Each is a
checkbox: check it when the decision is made and the fix lands.

Severity: 🔴 Critical · 🟠 Major · 🟡 Minor. "Sweep fix" = what this branch did.

---

## Authentication & key storage

- [ ] **A5** 🟠 File-fallback API key derives from world-readable `/etc/machine-id`.
  The machine-id is readable by any local process/user, so the "encryption" of
  the fallback key file is obfuscation, not protection, on multi-user machines.
  **Decision needed:** OS keychain only (drop the file fallback)? Or
  key-derivation bound to a user-secret (e.g. login password via keyutils)?
  Sweep fix: none (needs product decision).

## Companion / Telegram / cloud

- [ ] **B2** 🟠 Companion defaults to LAN bind, but the
  `assert_control_plane_bind()` helper (from the rejected tunnel work) wants
  loopback-only. LAN mode currently logs a loud warning and proceeds (sweep
  fix). **Decision needed:** is LAN-without-TLS an accepted product posture,
  or must LAN mode require the device-credential + TLS?
- [ ] **B3** 🟠 Cloud deploy docs show `0.0.0.0` with no TLS and no bind
  assertion. Sweep fix: `serve()` now requires `ZEN_CLOUD_PUBLIC=1` to bind
  non-loopback. **Decision needed:** bless a TLS story (reverse-proxy guide?
  built-in ACME?) and update the docs.
- [ ] **B6** 🟡 Telegram pairing from group chats: a pairing code posted in a
  group could be redeemed by any member. **Decision needed:** restrict pairing
  to 1:1 DMs with the bot, or keep group pairing as a feature?
- [ ] **B20** 🟡 Public-IP bind override exists as an escape hatch.
  **Decision needed:** keep with warning, or remove the override entirely?

## MCP & skills (prompt-injection surface)

- [ ] **C3** 🟠 No sanitization or provenance marking for untrusted tool/skill
  content entering the model context. Sweep fixes closed the worst vectors
  (C1 spawn-at-open, C2 pre-approval descriptions) on their feature branches -
  see the "What the sweep fixed" record below - but there is no general
  provenance/sanitization layer on main. **Decision needed:** adopt a tagging scheme
  (e.g. wrap untrusted content in marked spans) as a follow-up epic.
- [ ] **C6** 🟡 The skill-library approval store (`approved` in the manifest)
  has no bridge to the skill registry's approval set - approving in one place
  may not be honored in the other. **Verify** the wiring when both PRs land.
- [ ] **C14** 🟠 The skill catalog invokes `npx -y <package>` **unpinned**.
  This is the exact Clinejection-class vector (R7's raison d'être): a
  compromised npm package runs at install time. **Decision needed:** pin to
  content hashes / a lockfile, or vendor the catalog packages.
- [ ] **N3** 🟡 MCP `allowed_tools` is parsed but enforcement lives in the
  permission system that doesn't exist yet. Wire it when approvals land.
- [ ] **N7** 🟡 Skill `requires_approval` frontmatter flag is parsed but only
  enforced inside `invoke_skill`; make sure every invocation path goes through it.

## Secret scrubbing (residual)

- [ ] **D2-entropy** 🟡 Bare secrets with no known prefix (random-looking
  strings) are not detected - entropy-based detection was deliberately left
  out (false-positive risk). **Decision needed:** accept the residual risk or
  scope an entropy detector with an allowlist.
- [ ] **D6** 🟡 (design) Cross-field scrubbing: a secret split across two
  transcript fields (e.g. key in one message, value in the next) is not
  correlated. Accept or design a session-wide pass.
- [ ] **D11-trust** 🟡 Re-scrub-on-ingest (sweep fix) assumes scrubbing is
  idempotent and that dropping <8-char entries on reload is acceptable.
  Confirm this matches the product's data-retention expectations.
- [ ] **D14** 🟡 Verified: the share-bundle HTML export escapes correctly
  (test `html_escapes_injection`); no action, kept for the record.

## Sandbox

- [ ] **D17** 🟡 Landlock `BestEffort` compatibility: on older kernels some
  rules silently downgrade. The spawn path now reports the child's attested
  level (sweep fix D16), but **policy authors** should know which rules are
  best-effort. Consider a `policy.audit()` that lists unenforceable rules.
- [ ] **D29** 🟡 Residual: seccomp is a blocklist, not an allowlist. A
  sufficiently novel kernel primitive bypasses it by definition. Accept as
  defense-in-depth (Landlock is the primary boundary).

## Automations & goal mode

- [ ] **D23** 🟠 Automation spec trust is undefined: specs live in
  `~/.zen/automations.json`, editable by any process as the user. A
  malicious spec = scheduled prompt injection. **Decision needed:** sign specs,
  or at minimum show a diff/approval when a spec file changes out-of-band.
- [ ] **D24** 🟠 `BudgetGuard` is cooperative and currently unwired in places;
  do not ship scheduled automations as "safe" until the guard is attached at
  every choke point. Gate the feature on the wiring checklist.
- [ ] **D25** 🟠 Scheduled runs have side effects (they run real agent
  sessions) despite "no auto-apply". The invariant is "no *unattended*
  mutation", not "no side effects". **Decision needed:** make the copy say
  this plainly, and consider a dry-run mode.
- [ ] **D31** 🟡 Automation notification bodies are truncated to 280 chars;
  truncation happens *after* scrubbing (sweep fix D27) - confirm ordering stays
  scrub-then-truncate in future edits.
- [ ] **D32** 🟡 Goal-mode verifier is an LLM: `parse_verifier_reply` now
  extracts citations (sweep fix D30) but does not *require* them. Consider
  failing verification when a "pass" cites no artifacts.

---

## What the sweep fixed (record - fixes land with their feature branches)

The R7 sweep branch fixed the items below, each with a regression test, but the
fixes target files that only exist on unmerged feature branches
(`share.rs`, `sandbox_v4.rs`, `skills.rs`, …). They are recorded here and will
land together with their respective feature branches - **this MR contains only
this checklist document**, no code changes.

A1, A2, A3, A4, A6, A7, A8, A9 · B1, B4, B5, B7, B8, B9, B10, B11, B12, B13, B14 ·
C1, C2, C4, C5, C7, C8, C9, C10, C11, C12 · D1, D2, D3, D4, D5, D7, D10, D11, D13,
D15, D16, D18, D20, D21, D26, D27, D28, D30.
Supply-chain audit (OSV, 2026-10-17): no advisories on any engine dependency
(as of that date; re-run before release).
