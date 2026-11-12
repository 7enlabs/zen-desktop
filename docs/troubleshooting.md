# Troubleshooting

Common problems and fixes for building, testing, and running ZenCode Desktop.
Each entry: **symptom → likely cause → fix**.

## Build issues

### `cargo build` fails with "the lock file needs to be updated"
**Cause:** CI and docs use `--locked`; your `Cargo.toml` changed without updating `Cargo.lock`.
**Fix:** run `cargo update` (or `cargo update -p <crate>` for one crate), commit the updated `Cargo.lock`, then rebuild with `cargo build --locked`.

### Build fills the disk / `/tmp` runs out of space
**Cause:** `cargo` defaults `CARGO_TARGET_DIR` under the project; on small disks or tmpfs (`/tmp` is often a 512M tmpfs) a full workspace build can exhaust it.
**Fix:** point the target dir at a persistent location with room:
```sh
export CARGO_TARGET_DIR=~/workspace/zencode-target
cargo build --locked
```

### `rustc` version errors / missing components
**Cause:** the project pins the stable toolchain via rustup; a system `cargo` may be older or lack `clippy`/`rustfmt`.
**Fix:**
```sh
rustup toolchain install stable --profile minimal --component rustfmt,clippy
export PATH="$HOME/.cargo/bin:$PATH"
cargo --version  # verify it resolves to the rustup shim
```

## Test issues

### Snapshot test fails with a `duration_ms` (or other timing) diff
**Cause:** insta snapshots must be deterministic; wall-clock values like `duration_ms` differ on every machine and CI run. (This flaked CI on all 3 OS once.)
**Fix:** scrub nondeterministic fields before snapshotting - e.g. redact `"duration_ms":<n>` to `"duration_ms":0` in the test, then re-accept the snapshot with `cargo insta review` (or `INSTA_UPDATE=always cargo test` once, then verify the diff).

### `fake_llm` test hangs or fails to bind
**Cause:** the test spins up a scripted OpenAI-compatible server on `127.0.0.1:0` (OS-assigned port). A hang usually means a firewall/sandbox blocks localhost TCP, or a previous test process still holds resources.
**Fix:** check nothing else is listening (`ss -tlnp | grep 127.0.0.1`), kill stale test processes, and re-run just that test:
```sh
cd engine && cargo test --locked --test fake_llm
```

### Tests pass locally but fail on CI
**Cause:** CI runs each crate on ubuntu/macos/windows. Common culprits: path separators (`/` vs `\`), line endings (CRLF), case-sensitive filesystems, timezone/locale differences.
**Fix:** check *which* OS failed in the Actions tab first, then reproduce the likely cause locally (e.g. `git config core.autocrlf` issues, `Path::join` instead of string concat).

## Model / API issues

### HTTP 429 (rate limit) on `:free` models
**Cause:** free tiers are aggressively rate-limited (e.g. OpenRouter `:free` ≈ 50 req/day; Kilo anonymous ≈ 200 req/hour/IP).
**Fix:** the CLI's client already retries 429/5xx honoring `Retry-After` and walks `fallback_models`. If you still hit limits: wait for the reset window, reduce request volume, or switch provider. Do not hammer retries - it extends the ban.

### `401 Unauthorized` / `403` from a provider
**Cause:** missing, expired, or revoked API key; or the key lacks scope for the model.
**Fix:** re-check the key with a harmless read (`GET /v1/models`), confirm the env var name matches your provider config, and never paste keys into chat/logs.

### Unhelpful error deep inside the HTTP client (empty `base_url` / `model`)
**Cause:** `RunOptions.base_url` and `model` are plain strings with no early validation on `main`; an empty value fails deep inside the client with a cryptic message.
**Fix:** double-check both fields before calling `run_task` - `base_url` must be a full `https://…/v1` URL, `model` must be a non-empty model id (free-only: must end with `:free`).

### Model returns empty or truncated output
**Cause:** `max_tokens` too low, or the provider cut the response.
**Fix:** raise `max_tokens` in `RunOptions`; check `finish_reason` in the raw response (`length` = hit the cap).

## Platform issues

### Tauri app shows a blank window on Linux
**Cause:** Tauri 2 on Linux needs WebKitGTK system libraries, which are not always installed.
**Fix:** install the distro's webkit2gtk packages, e.g. on Debian/Ubuntu:
```sh
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```
then rebuild.

### Windows: paths with `\` break tool calls or session files
**Cause:** string-concatenated paths assume `/`.
**Fix:** always build paths with `std::path::Path::join`; in tests, use `tempfile::tempdir()` rather than hardcoded `/tmp`.

### macOS: "unidentified developer" warning on first launch
**Cause:** pre-release builds are unsigned (Apple signing is paid and deferred).
**Fix:** right-click → Open, or `xattr -d com.apple.quarantine <app>`. Expected until signing is set up.

## CI issues

### CI is red - where to start
1. Open the failed run in the Actions tab and note **which job/OS** failed (`lint` vs `test`, ubuntu vs macos vs windows).
2. `lint` failures are usually `cargo fmt --check` or `clippy -D warnings` - run both locally and fix.
3. `test` failures: read the failing test's log tail first; look for the patterns above (snapshot timing, OS-specific paths).
4. If only one OS fails, it's almost certainly platform-specific (paths, line endings, timing) - not logic.

### CI fails on `Cargo.lock` being out of date
**Cause:** a `Cargo.toml`/`Cargo.lock` mismatch (see build issues).
**Fix:** run `cargo update`, commit the lockfile, push again.

## Still stuck?

- Search existing issues on `7enlabs/zen-desktop` - someone may have hit it.
- For CLI-side problems (agent loop, stream-json), file an issue on `atzenn/zencode` with the `desktop` label.
- When reporting, include: OS, `rustc --version`, the exact command, and the last ~30 lines of output.
