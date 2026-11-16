use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::{Action, ActionType, NotificationExt};

pub const SESSION_ACTIONS_ID: &str = "zen-session";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    TaskFinished,
    TaskFailed,
    ApprovalNeeded,
    StillWorking,
}

#[derive(Debug, Default)]
struct RunState {
    notified: bool,
    last_approval: Option<Instant>,
    last_milestone: Option<Instant>,
}

pub struct NotificationManager {
    runs: Mutex<HashMap<String, RunState>>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
        }
    }

    pub fn should_notify<R: Runtime>(app: &AppHandle<R>) -> bool {
        match app.get_webview_window("main") {
            Some(window) => !window.is_focused().unwrap_or(false),

            None => true,
        }
    }

    fn can_notify_run(&self, session_id: &str, kind: NotificationKind) -> bool {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let state = runs.entry(session_id.to_string()).or_default();
        match kind {
            NotificationKind::ApprovalNeeded => {
                if let Some(last) = state.last_approval {
                    if last.elapsed() < Duration::from_secs(300) {
                        return false;
                    }
                }
                state.last_approval = Some(Instant::now());
                true
            }
            NotificationKind::StillWorking => {
                if let Some(last) = state.last_milestone {
                    if last.elapsed() < Duration::from_secs(900) {
                        return false;
                    }
                }
                state.last_milestone = Some(Instant::now());
                true
            }
            _ => {
                if state.notified {
                    return false;
                }
                state.notified = true;
                true
            }
        }
    }

    #[allow(dead_code)]
    pub fn clear_run(&self, session_id: &str) {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runs.remove(session_id);
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn register_actions<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let action_type = ActionType::builder(SESSION_ACTIONS_ID)
        .actions(vec![
            Action::builder("view", "View").foreground(true).build(),
            Action::builder("dismiss", "Dismiss").build(),
        ])
        .build();
    app.notification()
        .register_action_types(vec![action_type])
        .map_err(|e| e.to_string())
}

fn truncate(text: &str, max_chars: usize) -> String {
    let truncated: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        format!("{truncated}…")
    } else {
        truncated
    }
}

pub fn notify_session_end<R: Runtime>(
    app: &AppHandle<R>,
    manager: &NotificationManager,
    session_id: &str,
    summary: &str,
    success: bool,
) -> Result<(), String> {
    let kind = if success {
        NotificationKind::TaskFinished
    } else {
        NotificationKind::TaskFailed
    };
    if !manager.can_notify_run(session_id, kind) {
        return Ok(());
    }
    if !NotificationManager::should_notify(app) {
        return Ok(());
    }
    let title = if success {
        "Zen - task finished"
    } else {
        "Zen - task failed"
    };
    let body = format!("{} - {}", truncate(summary, 80), session_id);
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .action_type_id(SESSION_ACTIONS_ID)
        .silent()
        .show()
        .map_err(|e| e.to_string())
}

pub fn notify_approval_needed<R: Runtime>(
    app: &AppHandle<R>,
    manager: &NotificationManager,
    tool_name: &str,
    session_id: &str,
) -> Result<(), String> {
    if !manager.can_notify_run(session_id, NotificationKind::ApprovalNeeded) {
        return Ok(());
    }
    if !NotificationManager::should_notify(app) {
        return Ok(());
    }
    app.notification()
        .builder()
        .title("Zen - approval needed")
        .body(format!(
            "Agent is waiting for approval: `{}` ({})",
            truncate(tool_name, 60),
            session_id
        ))
        .action_type_id(SESSION_ACTIONS_ID)

        .show()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cmd_notify_still_working<R: Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, NotificationManager>,
    session_id: String,
    elapsed_mins: u64,
) -> Result<(), String> {
    if !manager.can_notify_run(&session_id, NotificationKind::StillWorking) {
        return Ok(());
    }
    if !NotificationManager::should_notify(&app) {
        return Ok(());
    }
    app.notification()
        .builder()
        .title("Zen - still working…")
        .body(format!(
            "Run {} has been going for {} min - no action needed.",
            session_id, elapsed_mins
        ))
        .silent()
        .show()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cmd_notify_session_end<R: Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, NotificationManager>,
    session_id: String,
    summary: String,
    success: bool,
) -> Result<(), String> {
    notify_session_end(&app, &manager, &session_id, &summary, success)
}

#[tauri::command]
pub fn cmd_notify_approval_needed<R: Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, NotificationManager>,
    tool_name: String,
    session_id: String,
) -> Result<(), String> {
    notify_approval_needed(&app, &manager, &tool_name, &session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_one_notification_per_run() {
        let manager = NotificationManager::new();
        assert!(manager.can_notify_run("s1", NotificationKind::TaskFinished));
        assert!(!manager.can_notify_run("s1", NotificationKind::TaskFinished));

        assert!(manager.can_notify_run("s2", NotificationKind::TaskFailed));
    }

    #[test]
    fn approval_needed_throttled_to_5min() {
        let manager = NotificationManager::new();
        assert!(manager.can_notify_run("s1", NotificationKind::ApprovalNeeded));

        assert!(!manager.can_notify_run("s1", NotificationKind::ApprovalNeeded));
    }

    #[test]
    fn still_working_throttled_to_15min() {
        let manager = NotificationManager::new();
        assert!(manager.can_notify_run("s1", NotificationKind::StillWorking));

        assert!(!manager.can_notify_run("s1", NotificationKind::StillWorking));

        assert!(manager.can_notify_run("s1", NotificationKind::TaskFinished));
    }

    #[test]
    fn clear_run_resets_state() {
        let manager = NotificationManager::new();
        assert!(manager.can_notify_run("s1", NotificationKind::TaskFinished));
        assert!(!manager.can_notify_run("s1", NotificationKind::TaskFinished));
        manager.clear_run("s1");
        assert!(manager.can_notify_run("s1", NotificationKind::TaskFinished));
    }

    #[test]
    fn truncate_short_text_unchanged() {
        assert_eq!(truncate("hello", 80), "hello");
    }

    #[test]
    fn truncate_long_text_gets_ellipsis() {
        let long = "a".repeat(100);
        let out = truncate(&long, 80);
        assert_eq!(out.chars().count(), 81);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_unicode_safe() {

        let s = "héllo wörld - test ✓";
        let out = truncate(s, 8);
        assert!(out.chars().count() <= 9);
    }
}
