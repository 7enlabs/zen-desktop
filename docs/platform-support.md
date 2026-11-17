# Platform Support Matrix - Zen Desktop

**Last verified:** 2026-10-17 (engine, `main` @ `eb5d364`; sandbox PR branches as noted).
**Policy:** Zen Desktop ships on Linux, macOS, and Windows. Every PR must keep
all three building. See `scripts/check-cross.sh` for the local cross-compile check.

## Build status

| Component | Linux (x86_64) | macOS (aarch64) | Windows (x86_64) |
|---|---|---|---|
| `engine` (main) | ✅ native CI | ✅ native CI | ✅ native CI |
| `engine` cross-check from Linux | n/a (native) | ⚠️ needs macOS CI (no SDK on Linux) | ✅ via `x86_64-pc-windows-gnu` + mingw |
| `rebuild` (cowsay pilot) | ✅ | ✅ | ✅ |
| `core` (frozen pilot) | ✅ | ✅ | ✅ |
| Tauri shell (`desktop/`, PR #47) | ⏳ PR open | ⏳ PR open | ⏳ PR open |

Legend: ✅ verified · ⚠️ must be covered by CI · ⏳ not yet merged

## Sandbox backends (PRs #41 / #44 / #50 - all open, awaiting review)

| OS | Backend | Enforcement | Honest reporting |
|---|---|---|---|
| Linux | Landlock ABI v1 (`landlock = "0.4"`, Linux-gated dep) | Kernel ruleset on current thread | `Full` only when kernel confirms; else `Partial` |
| macOS | `sandbox-exec` Seatbelt profile (child process) | Child process only | `isolate()` → `None`; `spawn_sandboxed` → `Partial` max |
| Windows | Job Objects (`CreateJobObjectW`, raw FFI, zero deps) | Child process resource limits | `isolate()` → `None`; `spawn_sandboxed` → `Partial` max |

All three implement the same `Sandbox` trait (`SandboxPolicy`, `Isolation`).
No backend ever claims `Full` without kernel confirmation.

### Cross-compile verification (2026-10-17)

- **Windows FFI (`sandbox_windows.rs`)**: first-ever compile check via
  `cargo check --target x86_64-pc-windows-gnu` (mingw-w64) - **passes**.
  Fixed during this audit: unused `c_void` import, needless `return`
  (both would fail `clippy -D warnings`). Pushed to `lead/windows-sandbox`.
- **macOS (`sandbox_macos.rs`)**: cannot be compile-checked from Linux
  (no macOS SDK; `cc` rejects `-arch`/`-mmacosx-version-min`). The
  `#[cfg]` gates were reviewed by reading: macOS-only code is behind
  `#[cfg(target_os = "macos")]`, with a non-executing fallback on other OS.
  **macOS CI must validate before merge.**
- **Linux (Landlock)**: `landlock = "0.4"` is correctly gated behind
  `[target.'cfg(target_os = "linux")'.dependencies]`; the
  `LandlockSandbox` struct/impls/tests are behind `#[cfg(target_os = "linux")]`.
  Verified on the `lead/*-sandbox` branches.

## Tauri shell (PR #47, open)

- 14 of 15 day-one plugins registered. `tauri-plugin-deep-link` is
  **excluded**: v2.6.2 does not compile (requires `rust-ini` 0.21 but
  constructs `ParseOption` without the new fields - upstream bug).
  The `zen://` scheme stays declared in `tauri.conf.json` for re-adding later.
- `tauri-plugin-autostart` uses `MacosLauncher::LaunchAgent` (correct per-OS behavior).
- `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` hides the
  console on Windows release builds. Correct.
- **Updater pubkey is a placeholder** - the real minisign keypair
  (`tauri signer generate -w`) MUST be generated and committed before the
  first public release. Shipping the placeholder means updates can never verify.
- Bundle targets: `"all"` (deb/rpm/AppImage, dmg, msi/nsis).

## Known limitations

1. **macOS cross-compile from Linux is impossible** without osxcross + macOS SDK.
   Native macOS CI is the only gate. Do not merge macOS-specific code without it.
2. **Windows MSVC target** (`x86_64-pc-windows-msvc`) cannot be checked from Linux
   for crates using `ring`/C code; the GNU target (`x86_64-pc-windows-gnu` +
   mingw-w64) is the supported local cross-check. Native Windows CI covers MSVC.
3. **`sandbox-exec` profiles are best-effort.** Seatbelt syntax is not validated
   at build time; review generated profiles before trusting them.
4. **Landlock needs kernel ≥ 5.13** (ABI v1). Older kernels get `Partial`
   (in-process checks only) - never silently `Full`.

## For contributors

```bash
# From the repo root - checks what can be checked locally:
./scripts/check-cross.sh
```

What it does: `cargo fmt --check`, `cargo clippy -D warnings` (native),
`cargo check --target x86_64-pc-windows-gnu` for `engine/` (installs the
target + mingw if missing), and prints the macOS reminder.
What it cannot do: macOS target check (needs Apple SDK) - that is CI's job.
