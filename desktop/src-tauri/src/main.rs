#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod notifications;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use notifications::NotificationManager;
use tauri::ipc::Channel;
use tauri::Emitter;
use tauri::Manager;
use tauri::menu::{MenuBuilder, MenuItem, SubmenuBuilder};
use zencode_engine::{
    checkpoints::CheckpointManager,
    events::parse_ndjson,
    ipc_bridge::{BridgeSink, IpcEnvelope},
    scope::ScopeDecl,
    session_store::{SessionMeta, SessionStore},
    timeline::{self, RestoreOutcome, TimelineCheckpoint, TimelineResponse},
    RunOptions,
};

struct AppState {
    sinks: Mutex<HashMap<String, Arc<BridgeSink>>>,
}

fn ensure_free_model(model: &str) -> Result<(), String> {
    let m = model.trim();
    if m.is_empty() {
        return Err("model must not be empty".to_owned());
    }
    if !m.ends_with(":free") {
        return Err(format!("only :free models are allowed (got {m:?})"));
    }
    Ok(())
}

struct ForwardingSink {
    bridge: Arc<BridgeSink>,
    channel: Channel<IpcEnvelope>,
}

impl zen_core::EventSink for ForwardingSink {
    fn emit(&self, ev: zen_core::AgentEvent) {
        let before = self.bridge.envelopes().len();
        self.bridge.emit(ev);
        let envs = self.bridge.envelopes();

        if envs.len() > before {
            let _ = self.channel.send(envs[envs.len() - 1].clone());
        }
    }
}

#[tauri::command]
async fn run_task(
    app: tauri::AppHandle,
    prompt: String,
    model: String,
    base_url: String,
    cwd: String,
    on_event: Channel<IpcEnvelope>,
) -> Result<String, String> {
    ensure_free_model(&model)?;
    if prompt.trim().is_empty() {
        return Err("prompt must not be empty".to_owned());
    }
    if base_url.trim().is_empty() {
        return Err("no LLM backend configured: set base_url to a free OpenAI-compatible endpoint (e.g. https://ai-gateway.kilo.ai/api/v1)".to_owned());
    }
    let cwd_path = PathBuf::from(&cwd);
    if !cwd_path.is_dir() {
        return Err(format!("cwd is not a directory: {cwd}"));
    }

    let session_id = SessionStore::new_session_id();
    let bridge = Arc::new(BridgeSink::new());
    app.state::<AppState>()
        .sinks
        .lock()
        .map_err(|e| format!("state lock poisoned: {e}"))?
        .insert(session_id.clone(), bridge.clone());

    let sink: Arc<dyn zen_core::EventSink> = Arc::new(ForwardingSink {
        bridge: bridge.clone(),
        channel: on_event,
    });
    let mut opts = RunOptions::new(base_url, model.clone(), cwd_path.clone());

    opts.scope = Some(ScopeDecl {
        files: vec![],
        dirs: vec![cwd_path.clone()],
        description: "user-selected working directory".to_owned(),
    });
    let sid = session_id.clone();

    tauri::async_runtime::spawn(async move {
        let result = zencode_engine::run_task_with_sink(&opts, &prompt, sink).await;
        if let Err(e) = &result {

            eprintln!("run_task {sid} failed: {e:#}");
        }

        if let Ok(store) = SessionStore::with_default_root() {
            let envelopes: Vec<_> = bridge
                .envelopes()
                .iter()
                .filter_map(|e| e.to_envelope().ok())
                .collect();
            let _ = store.save_session(&sid, &envelopes, None, &model, &cwd_path);
        }
    });

    Ok(session_id)
}

#[tauri::command]
fn list_sessions() -> Result<Vec<SessionMeta>, String> {
    let store = SessionStore::with_default_root().map_err(|e| e.to_string())?;
    store.list_sessions().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_log_since(
    app: tauri::AppHandle,
    session_id: String,
    since: u64,
) -> Result<Vec<IpcEnvelope>, String> {

    if session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
        || session_id.trim().is_empty()
    {
        return Err("invalid session id".to_owned());
    }

    if let Ok(sinks) = app.state::<AppState>().sinks.lock() {
        if let Some(bridge) = sinks.get(&session_id) {
            return Ok(bridge.get_log_since(since));
        }
    }

    let store = SessionStore::with_default_root().map_err(|e| e.to_string())?;
    let path = store.root().join(format!("{session_id}.ndjson"));
    let ndjson = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let bridge = BridgeSink::from_ndjson(&ndjson).map_err(|e| e.to_string())?;
    Ok(bridge.get_log_since(since))
}

fn validate_session_id_arg(session_id: &str) -> Result<(), String> {
    if session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
        || session_id.trim().is_empty()
    {
        return Err("invalid session id".to_owned());
    }
    Ok(())
}

fn validate_checkpoint_id_arg(checkpoint_id: &str) -> Result<(), String> {
    let id = checkpoint_id.trim();
    if id.len() < 4 || id.len() > 40 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid checkpoint id".to_owned());
    }
    Ok(())
}

fn session_cwd(session_id: &str) -> Result<PathBuf, String> {
    let store = SessionStore::with_default_root().map_err(|e| e.to_string())?;
    let meta = store
        .list_sessions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.id == session_id)
        .ok_or_else(|| "unknown session id".to_owned())?;
    if meta.cwd.trim().is_empty() {
        return Err("session has no workspace recorded".to_owned());
    }
    Ok(PathBuf::from(meta.cwd))
}

#[tauri::command]
fn get_timeline(session_id: String) -> Result<TimelineResponse, String> {
    validate_session_id_arg(&session_id)?;
    let store = SessionStore::with_default_root().map_err(|e| e.to_string())?;
    let path = store.root().join(format!("{session_id}.ndjson"));
    let ndjson = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let envelopes = parse_ndjson(&ndjson).map_err(|e| e.to_string())?;
    let entries = timeline::build_timeline(&envelopes);

    let checkpoints = match session_cwd(&session_id) {
        Ok(cwd) => CheckpointManager::new(&cwd)
            .and_then(|m| m.list())
            .map(|infos| {
                infos
                    .into_iter()
                    .map(|i| TimelineCheckpoint {
                        id: i.id,
                        message: i.message,
                        timestamp: i.timestamp,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => vec![],
    };

    Ok(TimelineResponse {
        session_id,
        entries,
        checkpoints,
    })
}

#[tauri::command]
fn restore_checkpoint(session_id: String, checkpoint_id: String) -> Result<RestoreOutcome, String> {
    validate_session_id_arg(&session_id)?;
    validate_checkpoint_id_arg(&checkpoint_id)?;
    let cwd = session_cwd(&session_id)?;
    let manager = CheckpointManager::new(&cwd).map_err(|e| e.to_string())?;
    let report = manager
        .restore(checkpoint_id.trim())
        .map_err(|e| e.to_string())?;
    Ok(RestoreOutcome {
        restored_files: report.restored_files,
        safety_checkpoint_id: report.new_checkpoint_id,
    })
}

fn signed_version_comparator(current: &str, release: tauri_plugin_updater::RemoteRelease) -> bool {
    let Some(target) = tauri_plugin_updater::target() else {
        return false;
    };
    let (Ok(url), Ok(signature)) = (release.download_url(&target), release.signature(&target))
    else {
        eprintln!("updater: no manifest entry for {target}; ignoring update");
        return false;
    };
    match zencode_engine::release_signing::check_update_offer(
        current,
        &release.version.to_string(),
        url.as_str(),
        signature,
    ) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("updater: refusing update {}: {e}", release.version);
            false
        }
    }
}


fn build_app_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu> {
    let file = SubmenuBuilder::new(app, "File")
        .item(&MenuItem::with_id(app, "new-session", "New Session", true, Some("CmdOrCtrl+N"))?)
        .item(&MenuItem::with_id(app, "focus-composer", "Focus Composer", true, Some("CmdOrCtrl+L"))?)
        .separator()
        .item(&MenuItem::with_id(app, "open-palette", "Command Palette", true, Some("CmdOrCtrl+K"))?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .text("copy-session-id", "Copy Session ID")
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&MenuItem::with_id(app, "toggle-sidebar", "Toggle Sidebar", true, Some("CmdOrCtrl+B"))?)
        .item(&MenuItem::with_id(app, "toggle-density", "Toggle Density", true, Some("CmdOrCtrl+Shift+D"))?)
        .separator()
        .item(&MenuItem::with_id(app, "tab-chat", "Chat Tab", true, Some("CmdOrCtrl+1"))?)
        .item(&MenuItem::with_id(app, "tab-timeline", "Timeline Tab", true, Some("CmdOrCtrl+2"))?)
        .item(&MenuItem::with_id(app, "tab-network", "Network Tab", true, Some("CmdOrCtrl+3"))?)
        .item(&MenuItem::with_id(app, "tab-terminal", "Terminal Tab", true, Some("CmdOrCtrl+4"))?)
        .separator()
        .text("toggle-inspector", "Toggle Inspector")
        .build()?;
    let session = SubmenuBuilder::new(app, "Session")
        .text("next-session", "Next Session")
        .text("prev-session", "Previous Session")
        .text("focus-search", "Search Sessions")
        .build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .text("about", "About ZenCode Desktop")
        .build()?;
    MenuBuilder::new(app)
        .item(&file)
        .item(&edit)
        .item(&view)
        .item(&session)
        .item(&help)
        .build()
}

fn main() {
    tauri::Builder::default()

        .plugin(
            tauri_plugin_updater::Builder::new()
                .default_version_comparator(|current, release| {
                    signed_version_comparator(&current.to_string(), release)
                })
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())

        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))

        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState {
            sinks: Mutex::new(HashMap::new()),
        })

        .manage(NotificationManager::new())
        .invoke_handler(tauri::generate_handler![
            run_task,
            list_sessions,
            get_log_since,
            get_timeline,
            restore_checkpoint,
            notifications::cmd_notify_session_end,
            notifications::cmd_notify_approval_needed,
            notifications::cmd_notify_still_working,
        ])
        .setup(|app| {

            if let Err(e) = notifications::register_actions(app.handle()) {
                eprintln!("warning: could not register notification actions: {e}");
            }
            match build_app_menu(app.handle()) {
                Ok(menu) => {
                    if let Err(e) = app.set_menu(menu) {
                        eprintln!("warning: could not set app menu: {e}");
                    }
                }
                Err(e) => eprintln!("warning: could not build app menu: {e}"),
            }
            Ok(())
        })
        .on_menu_event(|app, event| {
            let _ = app.emit("menu-command", event.id().as_ref());
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
