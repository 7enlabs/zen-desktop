//! Budget enforcement through the subprocess run loop (Unix only — the fake
//! `zen` is a shell script; Windows coverage is the Leader's step-8 PC run).
//! The budget is observed from the event stream: tool calls are counted as
//! they stream, output tokens are estimated live and replaced by the
//! authoritative usage from the `result` line.

#![cfg(unix)]

#[path = "common/fake_zen.rs"]
mod fake_zen;

use std::sync::Arc;

use zencode_engine::budget::TaskBudget;
use zencode_engine::cli_spawn::{run_cli_task_with_binary, CliRunHandle};
use zencode_engine::events::RecordingSink;

fn has_budget_event(sink: &RecordingSink) -> bool {
    sink.to_ndjson()
        .unwrap()
        .lines()
        .any(|l| l.contains("budget_exceeded"))
}

#[tokio::test]
async fn output_token_budget_stops_task_and_logs_event() {
    let dir = fake_zen::tmpdir("tokens");
    let lines = [
        r#"{"v":1,"type":"system","subtype":"init","session_id":"00000000-0000-4000-8000-000000000001","tools":[],"model":"mock-model:free"}"#,
        r#"{"v":1,"type":"assistant","message":{"role":"assistant","content":"done"}}"#,
        r#"{"v":1,"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"00000000-0000-4000-8000-000000000001","num_turns":1,"usage":{"input_tokens":30,"output_tokens":60000},"model":"mock-model:free"}"#,
    ]
    .join("\n");
    let binary = fake_zen::stream_script(&dir, &lines);
    let sink = Arc::new(RecordingSink::new());
    let mut opts = fake_zen::base_opts(&dir);
    opts.budget = Some(TaskBudget {
        max_output_tokens: 50_000,
        ..TaskBudget::unlimited()
    });
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(&opts, "say hi", sink.clone(), &handle, Some(binary))
        .await
        .unwrap();
    assert!(!outcome.success, "budget trip must end the task as failed");
    assert!(
        outcome
            .result
            .contains("Task stopped: output token budget 50,000 exceeded (used 60,000)"),
        "unexpected result: {}",
        outcome.result
    );
    assert!(
        has_budget_event(&sink),
        "budget_exceeded missing from session log"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn tool_call_budget_denies_and_stops_task() {
    let dir = fake_zen::tmpdir("tools");
    let lines = [
        r#"{"v":1,"type":"system","subtype":"init","session_id":"00000000-0000-4000-8000-000000000001","tools":["Read"],"model":"mock-model:free"}"#,
        r#"{"v":1,"type":"assistant","message":{"role":"assistant","tool_calls":[{"id":"c1","type":"function","function":{"name":"Read","arguments":"{}"}},{"id":"c2","type":"function","function":{"name":"Read","arguments":"{}"}}]}}"#,
        r#"{"v":1,"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"00000000-0000-4000-8000-000000000001","num_turns":1,"usage":{"input_tokens":30,"output_tokens":5},"model":"mock-model:free"}"#,
    ]
    .join("\n");
    let binary = fake_zen::stream_script(&dir, &lines);
    let sink = Arc::new(RecordingSink::new());
    let mut opts = fake_zen::base_opts(&dir);
    opts.budget = Some(TaskBudget {
        max_tool_calls: 1,
        ..TaskBudget::unlimited()
    });
    let handle = CliRunHandle::new();
    let outcome =
        run_cli_task_with_binary(&opts, "read twice", sink.clone(), &handle, Some(binary))
            .await
            .unwrap();
    assert!(!outcome.success, "budget trip must end the task as failed");
    assert!(
        outcome
            .result
            .contains("Task stopped: tool call budget 1 exceeded (used 2)"),
        "unexpected result: {}",
        outcome.result
    );
    assert!(
        has_budget_event(&sink),
        "budget_exceeded missing from session log"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn generous_budget_lets_small_task_finish_cleanly() {
    let dir = fake_zen::tmpdir("ok");
    let binary = fake_zen::stream_script(&dir, &fake_zen::ok_stream());
    let sink = Arc::new(RecordingSink::new());
    let mut opts = fake_zen::base_opts(&dir);
    opts.budget = Some(TaskBudget::default());
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(&opts, "say hi", sink.clone(), &handle, Some(binary))
        .await
        .unwrap();
    assert!(outcome.success, "{}", outcome.result);
    assert_eq!(outcome.result, "hello");
    assert!(
        !has_budget_event(&sink),
        "no budget event should be logged within budget"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
