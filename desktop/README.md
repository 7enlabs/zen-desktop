# Zen Desktop - Tauri Shell (g6, engine wired)

Minimal Tauri 2 application shell with the engine connected via Tauri
commands (`run_task`, `list_sessions`, `get_log_since`) and event streaming
over `tauri::ipc::Channel` (D1/D2 designs).

## Layout

```
desktop/
├── src/
│   ├── index.html            # chat UI shell
│   ├── app.js                # live mode (Tauri) with demo.ndjson fallback
│   ├── demo.ndjson           # demo conversation for standalone viewing
│   └── styles.css            # dark theme
├── src-tauri/
│   ├── Cargo.toml            # tauri 2 + 15 plugins + zen_engine (path)
│   ├── build.rs
│   ├── tauri.conf.json       # window, bundle, updater + deep-link config
│   ├── capabilities/
│   │   └── default.json      # least-privilege permissions (default-deny base)
│   └── src/
│       └── main.rs           # plugin registration + engine command wiring
└── README.md                 # this file
```

## Engine wiring

`main.rs` exposes three commands:

| Command | Args | Returns |
|---------|------|---------|
| `run_task` | `prompt`, `model` (`:free` only), `base_url`, `cwd`, `on_event: Channel` | `session_id` (run continues in background) |
| `list_sessions` | - | `Vec<SessionMeta>` (newest first) |
| `get_log_since` | `session_id`, `since: u64` | `Vec<IpcEnvelope>` with `seq > since` |

`run_task` refuses non-`:free` models and empty `base_url` with a clear
error - nothing is faked. Finished runs are persisted via `SessionStore`.

## Plugins

The D3 design (`docs/research/d3-plugins-updater-2026-10-17.md`, PR #18)
specifies 11 day-one plugins. The g6 scaffold spec additionally requires
`autostart`, `os`, `clipboard-manager`, and `positioner`. This scaffold
registers the **union (15)** - Z App may trim:

| Plugin | Purpose |
|---|---|
| updater | auto-update (keypair still TODO - see below) |
| dialog | open/save file dialogs |
| fs | scoped filesystem access |
| notification | task-complete toasts (D10) |
| process | relaunch after update, exit |
| shell | spawn `zen` sidecar / open URLs |
| store | persist settings (model picker, theme) |
| log | frontend + backend logs to disk |
| window-state | remember window size/position |
| single-instance | focus existing window on second launch |
| deep-link | `zen://` URLs |
| autostart | launch at login (opt-in, D11) |
| os | platform/arch info |
| clipboard-manager | clipboard read/write |
| positioner | tray-window positioning (D11) |

> **Note:** `tauri-plugin-deep-link` (D3 day-one) is excluded from this
> scaffold: deep-link 2.6.2 (latest 2.x) does not compile - it requires
> `rust-ini 0.21` but constructs `ini::ParseOption` without the fields added
> in 0.21 (E0063). The `zen://` scheme stays declared in
> `tauri.conf.json`; re-add the plugin when upstream ships a fixed 2.x.

## Before the first public release (D3 checklist)

- [ ] `tauri signer generate -w` → minisign keypair. Private key → CI secret
      `TAURI_SIGNING_PRIVATE_KEY`. **Replace the `PLACEHOLDER` pubkey in
      `tauri.conf.json`** - shipping with the placeholder means updates can
      never be verified, and every install is stuck on manual downloads.
- [ ] Real app icons in `src-tauri/icons/` (currently solid-color PNG
      placeholders + stub .icns/.ico - replace with real branded icons
      before `tauri build`; `cargo check` only needs the files to exist).
- [ ] `tauri-action` workflow for signed per-OS builds (Z App - needs
      `workflow` scope; the 7enlab token lacks it).

## What's next (engine wiring)

1. Add `zen_engine` as a dependency (path or git) and expose Tauri
   commands (`run_task`, `cancel`, session list/search) per the D2 IPC design.
2. Stream `DesktopEvent`s to the frontend via `tauri::ipc::Channel`
   (D1 `StreamEmitter` design) with rAF batching.
3. Replace the placeholder `index.html` with the real UI.
4. Tighten `capabilities/default.json`: scope `fs` to workspace dirs, drop
   unused plugin permissions.

## Verify

```sh
cd desktop/src-tauri
cargo check
```

Full `tauri build` needs system WebKitGTK/WebView2 deps and is done in CI.
