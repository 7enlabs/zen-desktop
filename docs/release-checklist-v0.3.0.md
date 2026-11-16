# v0.3.0 Release Checklist

Tag v0.3.0 per PLAN.md when: **g5 eval ≥5/10 passes + CI green on 3 OS**.
Owner of merges: Z App (ZD Leader never merges). All PRs below are open on `7enlabs/zen-desktop`.

## 1. Pre-conditions - PRs that must merge first

| Roadmap step | PR | What it proves |
|---|---|---|
| g1 | (on main) | engine + fake-LLM tests, CI 3 OS |
| s1 | (on main, `eb5d364`) | event schema v1 + contract tests |
| g2 | #3 `lead/g2-smoke` | real `:free` run, 2 turns, $0.0 |
| g2+g5 | #4 `lead/claude-learnings` | `zen-desktop run`/`eval` binary, eval 10/10 pass |
| g4 | #6 `lead/g4-prompt-budget` | prompt ≤1200 tokens, fail-fast |
| g3 | #33 `lead/g3-rategate` | daily quota + model fallback |
| g3 infra | #8 `lead/p2-pretooluse-hook` | PreToolUse hook point |
| base | #1 `lead/rev-bump-04b742e` | CLI rev 04b742e + `Unknown` event arm |

Merge order: #1 → #3 → #4 → #6 → #8 → #33. Docs-only PRs (#2, #5, #7, #10–#22, #24, #25, #27–#29, #38) may merge any time; code PRs #23, #26, #30–#32, #34–#37, #39–#41 are v0.4.0/v0.5.0 scope and must NOT block v0.3.0.

Gate checks before tagging:
- [ ] `zen-desktop eval` ≥5/10 pass on a real `:free` model (rerun if CLI rev changed)
- [ ] CI green on linux/windows/macos for the merge commit
- [ ] `cargo audit` clean, `cargo deny check` clean

## 2. Blockers to resolve

- [ ] **Z App review queue** - 41 open PRs; the 8 in §1 are release-critical
- [ ] **atzenn/zencode#10** (RateGate placement) - #33 is engine-side; if CLI team claims it, drop #33 from the release
- [ ] **PR #4 CI fix** - commit `4715479` (`scrub_duration_ms`) must be pushed to `lead/claude-learnings` or the insta snapshot flakes on CI
- [ ] **PR #17 workflow push** - the supply-chain CI job commit exists locally only; needs a token with `workflow` scope (Z App) or it stays out of v0.3.0

## 3. Release steps

1. Bump `engine/Cargo.toml`: `0.3.0-dev` → `0.3.0`. (`core` 0.1.0 and `rebuild` 0.2.0 keep their own versions.)
2. CHANGELOG entry under `## [0.3.0] - YYYY-MM-DD`: g2/g3/g4/g5 features, `:free`-only policy, eval results.
3. `git tag -a v0.3.0 -m "v0.3.0: agent engine on Zen CLI"` and push the tag - the existing `release.yml` builds binaries for linux-x86_64, macos-arm64, windows-x86_64.
4. GitHub Release notes template:
   > **v0.3.0** - `zen-desktop run` + `zen-desktop eval` on free-only models.
   > Includes: prompt budget (1200 tokens), RateGate daily quota, session persistence.
   > Requires no API keys for `:free` models. See `docs/troubleshooting.md`.

## 4. D3 requirement - updater keypair BEFORE v0.3.0

Per `docs/research/d3-plugins-updater-2026-10-17.md` (PR #18): `tauri-plugin-updater`
verifies artifacts against a public key baked into the binary. Shipping v0.3.0
without the real keypair strands every install on manual downloads forever.

- [ ] Zen runs `tauri signer generate -w` **once**, offline
- [ ] Private key → GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY` (never in git)
- [ ] Public key → `tauri.conf.json` (`plugins.updater.pubkey`); fingerprint recorded in `docs/` + Notion

## 5. Post-release - v0.4.0 starts with

Already implemented (awaiting review, merge after the tag): #31 checkpoints,
#32 verify loop, #34 atomic writes, #36 plan mode, #39 compaction,
#40 parallel tools. Then: Tauri shell wiring (g6) per D1/D2 designs.
