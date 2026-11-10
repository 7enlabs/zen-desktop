//! Convergence guard through the subprocess run loop (Unix only — the fake
//! `zen` is a shell script; Windows coverage is the Leader's step-8 PC run).
//! The guard observes the event stream; the first detected loop stops the
//! child process and the pattern is reported. Re-plan injection into the
//! agent loop is gone by design: the loop lives in the CLI process, across
//! the process boundary, so the Desktop cannot inject a new prompt into it.

#![cfg(unix)]

#[path = "common/fake_zen.rs"]
mod fake_zen;

use std::sync::Arc;

use zencode_engine::cli_spawn::{run_cli_task_with_binary, CliRunHandle};
use zencode_engine::convergence::ConvergencePolicy;
use zencode_engine::events::RecordingSink;

fn count(ndjson: &str, needle: &str) -> usize {
    ndjson.lines().filter(|l| l.contains(needle)).count()
}

fn edit_assistant(id: &str) -> String {
    format!(
        r#"{{"v":1,"type":"assistant","message":{{"role":"assistant","tool_calls":[{{"id":"{id}","type":"function","function":{{"name":"Edit","arguments":"{{\"file_path\": \"src/main.rs\", \"old_string\": \"a\", \"new_string\": \"b\"}}"}}}}]}}}}"#
    )
}

fn bash_assistant(id: &str) -> String {
    format!(
        r#"{{"v":1,"type":"assistant","message":{{"role":"assistant","tool_calls":[{{"id":"{id}","type":"function","function":{{"name":"Bash","arguments":"{{\"command\": \"cargo test\"}}"}}}}]}}}}"#
    )
}

fn tool_result(id: &str, content: &str, is_error: bool) -> String {
    format!(
        r#"{{"v":1,"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{content}","is_error":{is_error}}}]}}}}"#
    )
}

fn init_line() -> String {
    r#"{"v":1,"type":"system","subtype":"init","session_id":"00000000-0000-4000-8000-000000000001","tools":["Edit","Bash"],"model":"mock-model:free"}"#.to_string()
}

fn result_line(text: &str) -> String {
    format!(
        r#"{{"v":1,"type":"result","subtype":"success","is_error":false,"result":"{text}","session_id":"00000000-0000-4000-8000-000000000001","num_turns":3,"usage":{{"input_tokens":30,"output_tokens":5}},"model":"mock-model:free"}}"#
    )
}

#[tokio::test]
async fn looping_edits_stop_the_run() {
    let dir = fake_zen::tmpdir("loop");
    // 3 rounds of Edit(src/main.rs) + failing Bash verify.
    let mut lines = vec![init_line()];
    for i in 1..=3 {
        lines.push(edit_assistant(&format!("e{i}")));
        lines.push(tool_result(&format!("e{i}"), "edit ok", false));
        lines.push(bash_assistant(&format!("b{i}")));
        lines.push(tool_result(
            &format!("b{i}"),
            "error[E0425]: cannot find value `x`",
            true,
        ));
    }
    lines.push(result_line("Build fixed."));
    let binary = fake_zen::stream_script(&dir, &lines.join("\n"));
    let sink = Arc::new(RecordingSink::new());
    let mut opts = fake_zen::base_opts(&dir);
    opts.convergence = Some(ConvergencePolicy::default());
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(
        &opts,
        "make the build pass",
        sink.clone(),
        &handle,
        Some(binary),
    )
    .await
    .unwrap();
    assert!(!outcome.success, "guard must end the task as failed");
    assert!(
        outcome.result.contains("convergence guard"),
        "result names the guard: {}",
        outcome.result
    );
    assert!(outcome.result.contains("src/main.rs"), "{}", outcome.result);
    let log = sink.to_ndjson().unwrap();
    assert_eq!(count(&log, "loop_detected"), 1, "{log}");
    assert_eq!(count(&log, "convergence_stop"), 1, "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn guard_is_opt_in_and_quiet_on_progress() {
    assert!(fake_zen::base_opts(&std::env::temp_dir())
        .convergence
        .is_none());

    let dir = fake_zen::tmpdir("progress");
    let mut lines = vec![init_line()];
    lines.push(edit_assistant("e1"));
    lines.push(tool_result("e1", "edit ok", false));
    lines.push(bash_assistant("b1"));
    lines.push(tool_result("b1", "test result: ok", false));
    lines.push(result_line("Build fixed."));
    let binary = fake_zen::stream_script(&dir, &lines.join("\n"));
    let sink = Arc::new(RecordingSink::new());
    let mut opts = fake_zen::base_opts(&dir);
    opts.convergence = Some(ConvergencePolicy::default());
    let handle = CliRunHandle::new();
    let outcome = run_cli_task_with_binary(
        &opts,
        "make the build pass",
        sink.clone(),
        &handle,
        Some(binary),
    )
    .await
    .unwrap();
    assert!(outcome.success, "{}", outcome.result);
    assert_eq!(outcome.result, "Build fixed.");
    let log = sink.to_ndjson().unwrap();
    assert_eq!(count(&log, "loop_detected"), 0, "{log}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn repeating_the_same_probe_is_stagnant() {
    let dir = fake_zen::tmpdir("probe");
    // 6 identical Read rounds: the first output is new, the next 5 are
    // stagnant, so the guard fires on the final round.
    let mut lines = vec![init_line()];
    for i in 1..=6 {
        lines.push(format!(
            r#"{{"v":1,"type":"assistant","message":{{"role":"assistant","tool_calls":[{{"id":"r{i}","type":"function","function":{{"name":"Read","arguments":"{{\"file_path\": \"notes.txt\"}}"}}}}]}}}}"#
        ));
        lines.push(tool_result(&format!("r{i}"), "same content", false));
    }
    lines.push(result_line("Looked around."));
    let binary = fake_zen::stream_script(&dir, &lines.join("\n"));
    let sink = Arc::new(RecordingSink::new());
    let mut opts = fake_zen::base_opts(&dir);
    opts.convergence = Some(ConvergencePolicy::default());
    let handle = CliRunHandle::new();
    let outcome =
        run_cli_task_with_binary(&opts, "look around", sink.clone(), &handle, Some(binary))
            .await
            .unwrap();
    assert!(!outcome.success, "guard must end the task as failed");
    let log = sink.to_ndjson().unwrap();
    assert_eq!(count(&log, "loop_detected"), 1, "{log}");
    assert!(log.contains("stagnant"), "{log}");
    assert!(
        outcome.result.contains("convergence guard"),
        "{}",
        outcome.result
    );
    let _ = std::fs::remove_dir_all(&dir);
}
