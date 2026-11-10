//! Integration tests for the subprocess run loop (`cli_spawn`), using a fake
//! `zen` shell script as the child (Unix only — Windows coverage is the
//! Leader's step-8 PC run). The fake prints canned stream-json lines, sleeps,
//! or fails; it never touches a real model.

#![cfg(unix)]

#[path = "common/fake_zen.rs"]
mod fake_zen;

use std::sync::Arc;

use zencode_engine::budget::TaskBudget;
use zencode_engine::cli_spawn::{run_cli_task_with_binary, CliRunHandle};
use zencode_engine::events::{DesktopEvent, RecordingSink};
use zencode_engine::stream_json::CliExit;

fn session_end_count(sink: &RecordingSink) -> usize {
    sink.events()
        .iter()
        .filter(|e| matches!(e.event, DesktopEvent::SessionEnd { .. }))
        .count()
}

#[tokio::test]
async fn stream_runs_end_to_end() {
    let dir = fake_zen::tmpdir("stream");
    let binary = fake_zen::stream_script(&dir, &fake_zen::ok_stream());
    let sink = Arc::new(RecordingSink::new());
    let opts = fake_zen::base_opts(&dir);
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(&opts, "say hello", sink.clone(), &handle, Some(binary))
        .await
        .unwrap();
    assert_eq!(outcome.exit, CliExit::Success);
    assert_eq!(outcome.exit_code, 0);
    assert!(outcome.success, "{}", outcome.result);
    assert_eq!(outcome.result, "hello");
    assert_eq!(outcome.num_turns, 1);
    assert_eq!(outcome.input_tokens, 7);
    assert_eq!(outcome.output_tokens, 3);
    assert_eq!(outcome.model, "mock-model:free");
    assert!(outcome.stop_reason.is_none());
    assert!(!outcome.timed_out);
    assert!(!outcome.cli_session_id.is_empty());
    assert_eq!(session_end_count(&sink), 1, "SessionEnd exactly once");
    let kinds: Vec<&str> = sink
        .events()
        .iter()
        .map(|e| match &e.event {
            DesktopEvent::SystemMessage { .. } => "system",
            DesktopEvent::AssistantMessage { .. } => "assistant",
            DesktopEvent::SessionEnd { .. } => "session_end",
            _ => "other",
        })
        .collect();
    assert!(kinds.contains(&"system"), "{kinds:?}");
    assert!(kinds.contains(&"assistant"), "{kinds:?}");
    assert!(kinds.contains(&"session_end"), "{kinds:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cli_error_surfaces_stderr_and_synthesizes_session_end() {
    let dir = fake_zen::tmpdir("fail");
    let binary = fake_zen::fail_script(&dir, "boom: something broke", 1);
    let sink = Arc::new(RecordingSink::new());
    let opts = fake_zen::base_opts(&dir);
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(&opts, "hi", sink.clone(), &handle, Some(binary))
        .await
        .unwrap();
    assert_eq!(outcome.exit, CliExit::Error);
    assert!(!outcome.success);
    assert!(outcome.stderr_tail.contains("boom: something broke"));
    assert!(
        outcome.result.contains("CLI exited 1"),
        "{}",
        outcome.result
    );
    assert_eq!(
        session_end_count(&sink),
        1,
        "synthetic SessionEnd exactly once"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn user_stop_kills_sleeping_child() {
    let dir = fake_zen::tmpdir("stop");
    let binary = fake_zen::sleep_script(&dir);
    let sink = Arc::new(RecordingSink::new());
    let opts = fake_zen::base_opts(&dir);
    let handle = CliRunHandle::new();
    let handle2 = handle.clone();
    let run = tokio::spawn(async move {
        run_cli_task_with_binary(&opts, "hi", sink, &handle2, Some(binary)).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let pid = handle.pid();
    handle.request_stop("user stop");
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(15), run)
        .await
        .expect("run must finish after stop")
        .unwrap()
        .unwrap();
    assert_eq!(outcome.exit, CliExit::Interrupted, "SIGINT exit is 130");
    assert!(!outcome.success);
    assert_eq!(outcome.stop_reason.as_deref(), Some("user stop"));
    assert!(outcome.duration_ms < 15_000, "{}", outcome.duration_ms);
    let pid = pid.expect("child pid recorded");
    #[cfg(target_os = "linux")]
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "no leftover child process"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn timeout_kills_sleeping_child() {
    let dir = fake_zen::tmpdir("timeout");
    let binary = fake_zen::sleep_script(&dir);
    let sink = Arc::new(RecordingSink::new());
    let mut opts = fake_zen::base_opts(&dir);
    opts.budget = Some(TaskBudget {
        max_duration_secs: 1,
        ..TaskBudget::unlimited()
    });
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(&opts, "hi", sink.clone(), &handle, Some(binary))
        .await
        .unwrap();
    assert!(outcome.timed_out);
    assert_eq!(outcome.stop_reason.as_deref(), Some("timeout"));
    assert!(outcome.result.contains("timed out"), "{}", outcome.result);
    assert!(!outcome.success);
    assert!(outcome.duration_ms < 15_000, "{}", outcome.duration_ms);
    assert_eq!(
        session_end_count(&sink),
        1,
        "synthetic SessionEnd exactly once"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn fallback_retries_on_provider_failure() {
    let dir = fake_zen::tmpdir("fallback");
    let binary = fake_zen::counting_script(&dir, &fake_zen::ok_stream());
    let sink = Arc::new(RecordingSink::new());
    let mut opts = fake_zen::base_opts(&dir);
    opts.fallback_models = vec!["fallback-model:free".to_string()];
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(&opts, "hi", sink.clone(), &handle, Some(binary))
        .await
        .unwrap();
    assert!(outcome.success, "{}", outcome.result);
    assert_eq!(outcome.result, "hello");
    assert_eq!(
        std::fs::read_to_string(dir.join("state")).unwrap().trim(),
        "2",
        "the child must run twice (primary + fallback)"
    );
    assert!(
        sink.to_ndjson().unwrap().contains("cli_fallback_retry"),
        "retry must be logged"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn scope_violation_stops_run() {
    let dir = fake_zen::tmpdir("scope");
    let lines = [
        r#"{"v":1,"type":"system","subtype":"init","session_id":"00000000-0000-4000-8000-000000000001","tools":["Write"],"model":"mock-model:free"}"#,
        r#"{"v":1,"type":"assistant","message":{"role":"assistant","tool_calls":[{"id":"c1","type":"function","function":{"name":"Write","arguments":"{\"file_path\": \"/etc/passwd\", \"content\": \"x\"}"}}]}}"#,
        r#"{"v":1,"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"00000000-0000-4000-8000-000000000001","num_turns":1,"usage":{"input_tokens":7,"output_tokens":3},"model":"mock-model:free"}"#,
    ]
    .join("\n");
    let binary = fake_zen::stream_script(&dir, &lines);
    let sink = Arc::new(RecordingSink::new());
    let opts = fake_zen::base_opts(&dir);
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(&opts, "hi", sink.clone(), &handle, Some(binary))
        .await
        .unwrap();
    assert!(!outcome.success);
    assert!(outcome.result.contains("scope:"), "{}", outcome.result);
    assert!(
        outcome.result.contains("outside the declared scope"),
        "{}",
        outcome.result
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tmpdir_is_unique_per_call() {
    let first = fake_zen::tmpdir("unique");
    let second = fake_zen::tmpdir("unique");
    assert_ne!(first, second, "same tag must not reuse a directory");
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}
