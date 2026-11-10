//! Integration tests for the CLI stream-json v1 parser (DV-2).
//!
//! Fixtures live in `engine/tests/fixtures/stream_json/` as `.jsonl` files,
//! one scenario each. They encode the CLI/Desktop contract v1 per the
//! authoritative `docs/stream-json-v1.md` (CLI team, zen 0.1.0), reconciled
//! 2026-10-23: assistant `content` is a string (omitted when only calling
//! tools), tool calls are OpenAI-style `message.tool_calls` with JSON-encoded
//! `arguments`, and the `result` line carries `model`.

use std::path::PathBuf;

use zencode_engine::events::DesktopEvent;
use zencode_engine::stream_json::{
    interpret_exit_code, parse_stream_json, to_desktop_events, CliExit, CliLine,
};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stream_json")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

fn parse_fixture(name: &str) -> Vec<CliLine> {
    parse_stream_json(&fixture(name)).unwrap_or_else(|e| panic!("parse fixture {name}: {e}"))
}

fn parse_single(name: &str) -> CliLine {
    let lines = parse_fixture(name);
    assert_eq!(
        lines.len(),
        1,
        "fixture {name} should hold exactly one line"
    );
    lines.into_iter().next().unwrap()
}

fn project(name: &str) -> Vec<DesktopEvent> {
    parse_fixture(name)
        .iter()
        .flat_map(to_desktop_events)
        .collect()
}

#[test]
fn parses_system_init() {
    let line = parse_single("01_system_init.jsonl");
    assert_eq!(line.v, 1);
    assert_eq!(line.line_type, "system");
    let events = to_desktop_events(&line);
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SystemMessage { event, text } => {
            assert_eq!(event, "cli_system:init");
            assert!(text.contains("ses_abc123"), "{text}");
            assert!(text.contains("cohere/north-mini-code:free"), "{text}");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_assistant_text() {
    let events = project("02_assistant_text.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::AssistantMessage {
            text,
            tool_calls,
            parent_tool_use_id,
        } => {
            assert_eq!(text.as_deref(), Some("Tôi sẽ đọc file cấu hình trước."));
            assert!(tool_calls.is_empty());
            assert_eq!(parent_tool_use_id, &None);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_assistant_tool_use() {
    let events = project("03_assistant_tool_use.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::AssistantMessage {
            text, tool_calls, ..
        } => {
            assert_eq!(text.as_deref(), Some("Đọc file:"));
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call_read_01");
            assert_eq!(tool_calls[0].name, "read");
            assert!(
                tool_calls[0].arguments.contains("tauri.conf.json"),
                "{}",
                tool_calls[0].arguments
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_user_tool_result() {
    let events = project("04_user_tool_result.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "toolu_01");
            assert!(content.contains("zencode-desktop"), "{content}");
            assert!(!is_error);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_user_tool_result_error() {
    let events = project("05_user_tool_result_error.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "toolu_02");
            assert!(is_error);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_result_success() {
    let events = project("06_result_success.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd {
            success,
            result,
            num_turns,
            total_cost_usd,
            input_tokens,
            output_tokens,
            duration_ms,
            ..
        } => {
            assert!(success);
            assert_eq!(result, "Đã cập nhật cấu hình xong.");
            assert_eq!(*num_turns, 3);
            assert_eq!(*total_cost_usd, 0.0);
            assert_eq!(*input_tokens, 1200);
            assert_eq!(*output_tokens, 340);
            assert_eq!(*duration_ms, 45210);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_result_error() {
    let events = project("07_result_error.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd {
            success, result, ..
        } => {
            assert!(!success);
            assert!(result.contains("429"), "{result}");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_full_session_in_order() {
    let events = project("08_full_session.jsonl");
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            DesktopEvent::SystemMessage { .. } => "system",
            DesktopEvent::AssistantMessage { .. } => "assistant",
            DesktopEvent::ToolResult { .. } => "tool_result",
            DesktopEvent::SessionEnd { .. } => "session_end",
            other => panic!("unexpected event in full session: {other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "system",
            "assistant",
            "assistant",
            "tool_result",
            "session_end"
        ]
    );
    match events.last().unwrap() {
        DesktopEvent::SessionEnd {
            success, num_turns, ..
        } => {
            assert!(success);
            assert_eq!(*num_turns, 2);
        }
        other => panic!("unexpected final event: {other:?}"),
    }
}

#[test]
fn unknown_line_type_is_surfaced_not_dropped() {
    let line = parse_single("09_unknown_type.jsonl");
    assert_eq!(line.line_type, "heartbeat");
    let events = to_desktop_events(&line);
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::Unknown { variant } => assert!(variant.contains("heartbeat"), "{variant}"),
        other => panic!("unknown line type must surface as Unknown, got: {other:?}"),
    }
}

#[test]
fn rejects_unsupported_schema_version() {
    let err = parse_stream_json(&fixture("10_bad_version.jsonl")).unwrap_err();
    assert!(
        err.to_string().contains("unsupported schema version"),
        "{err}"
    );
}

#[test]
fn blank_lines_are_ignored() {
    let lines = parse_stream_json("\n\n  \n{\"v\":1,\"type\":\"system\",\"subtype\":\"init\"}\n\n")
        .unwrap();
    assert_eq!(lines.len(), 1);
}

#[test]
fn exit_codes_follow_contract_v1() {
    assert_eq!(interpret_exit_code(0), CliExit::Success);
    assert_eq!(interpret_exit_code(1), CliExit::Error);
    assert_eq!(interpret_exit_code(130), CliExit::Interrupted);
    assert_eq!(interpret_exit_code(143), CliExit::Terminated);
    assert_eq!(interpret_exit_code(2), CliExit::Other(2));
    assert_eq!(interpret_exit_code(-1), CliExit::Other(-1));
}

#[test]
fn parses_assistant_tool_calls_without_text() {
    let events = project("11_assistant_tool_calls_no_text.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::AssistantMessage {
            text, tool_calls, ..
        } => {
            assert_eq!(text, &None);
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "call_task_1");
            assert_eq!(tool_calls[0].name, "Task");
            assert!(
                tool_calls[0].arguments.contains("Explore"),
                "{}",
                tool_calls[0].arguments
            );
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_result_model() {
    let events = project("12_result_with_model.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd { success, model, .. } => {
            assert!(success);
            assert_eq!(model, "cohere/north-mini-code:free");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn result_without_model_defaults_to_empty() {
    let events = project("06_result_success.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd { model, .. } => assert_eq!(model, ""),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn empty_input_parses_to_no_lines() {
    let lines = parse_stream_json(&fixture("13_empty_input.jsonl")).unwrap();
    assert!(lines.is_empty());
}

#[test]
fn blank_lines_only_parses_to_no_lines() {
    let lines = parse_stream_json(&fixture("14_blank_lines_only.jsonl")).unwrap();
    assert!(lines.is_empty());
}

#[test]
fn malformed_json_line_is_rejected() {
    let err = parse_stream_json(&fixture("15_malformed_json.jsonl")).unwrap_err();
    assert!(err.to_string().contains("invalid JSON"), "{err}");
}

#[test]
fn missing_schema_version_is_rejected() {
    let err = parse_stream_json(&fixture("16_missing_version.jsonl")).unwrap_err();
    assert!(
        err.to_string().contains("unsupported schema version"),
        "{err}"
    );
}

#[test]
fn truncated_stream_fails_fast_on_the_cut_line() {
    let err = parse_stream_json(&fixture("17_truncated_stream.jsonl")).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("line 3"), "{msg}");
    assert!(msg.contains("invalid JSON"), "{msg}");
}

#[test]
fn final_line_without_trailing_newline_parses() {
    let line = parse_single("18_no_trailing_newline.jsonl");
    assert_eq!(line.v, 1);
    assert_eq!(line.line_type, "result");
}

#[test]
fn parses_assistant_multiple_tool_calls() {
    let events = project("19_multiple_tool_calls.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::AssistantMessage {
            text, tool_calls, ..
        } => {
            assert_eq!(text, &None);
            assert_eq!(tool_calls.len(), 2);
            assert_eq!(tool_calls[0].id, "call_read_02");
            assert_eq!(tool_calls[0].name, "read");
            assert_eq!(tool_calls[1].id, "call_bash_01");
            assert_eq!(tool_calls[1].name, "bash");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_result_error_with_model() {
    let events = project("20_result_error_with_model.jsonl");
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::SessionEnd {
            success,
            result,
            model,
            ..
        } => {
            assert!(!success);
            assert!(result.contains("429"), "{result}");
            assert_eq!(model, "cohere/north-mini-code:free");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn parses_user_two_tool_results_in_order() {
    let events = project("21_user_two_tool_results.jsonl");
    assert_eq!(events.len(), 2);
    match &events[0] {
        DesktopEvent::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "toolu_11");
            assert!(!is_error);
        }
        other => panic!("unexpected event: {other:?}"),
    }
    match &events[1] {
        DesktopEvent::ToolResult {
            tool_use_id,
            is_error,
            ..
        } => {
            assert_eq!(tool_use_id, "toolu_12");
            assert!(is_error);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

// Fixtures 23-26 are copies of the CLI repo's sample fixtures
// (7enlabs/zencode main 03eb057, docs/fixtures/stream-json/*.ndjson),
// kept verbatim so the Desktop parser is checked against the CLI's own
// examples. The CLI sample files use "<uuid>" / "<cwd>" placeholders.

#[test]
fn compact_boundary_maps_to_compact_boundary() {
    let events = project("23_compact_boundary.ndjson");
    let found = events.iter().find_map(|e| match e {
        DesktopEvent::CompactBoundary {
            trigger,
            pre_tokens,
        } => Some((trigger.clone(), *pre_tokens)),
        _ => None,
    });
    assert_eq!(
        found,
        Some(("auto".to_string(), 10)),
        "compact_boundary must project to CompactBoundary, got: {events:?}"
    );
}

#[test]
fn hook_message_stays_system_message_with_text() {
    let events = project("24_hook_message.ndjson");
    let found = events.iter().find_map(|e| match e {
        DesktopEvent::SystemMessage { event, text } if event == "cli_system:hook_message" => {
            Some(text.clone())
        }
        _ => None,
    });
    let text = found.expect("hook_message must project to a SystemMessage");
    assert!(text.contains("hook says hi"), "{text}");
    assert!(text.contains("PreToolUse"), "{text}");
}

#[test]
fn permission_denied_maps_to_permission_denied() {
    let events = project("25_permission_denied.ndjson");
    let found = events.iter().find_map(|e| match e {
        DesktopEvent::PermissionDenied {
            tool_name,
            tool_use_id,
        } => Some((tool_name.clone(), tool_use_id.clone())),
        _ => None,
    });
    assert_eq!(
        found,
        Some(("Write".to_string(), "call_write_1".to_string())),
        "permission_denied must project to PermissionDenied, got: {events:?}"
    );
}

#[test]
fn api_retry_maps_to_api_retry() {
    let events = project("26_api_retry.ndjson");
    let found = events.iter().find_map(|e| match e {
        DesktopEvent::ApiRetry {
            attempt,
            max_retries,
            retry_delay_ms,
            error_status,
            error,
        } => Some((
            *attempt,
            *max_retries,
            *retry_delay_ms,
            *error_status,
            error.clone(),
        )),
        _ => None,
    });
    let (attempt, max_retries, retry_delay_ms, error_status, error) =
        found.expect("api_retry must project to ApiRetry");
    assert_eq!(attempt, 1);
    assert_eq!(max_retries, 5);
    assert_eq!(retry_delay_ms, 1160);
    assert_eq!(error_status, Some(500));
    assert!(error.contains("upstream overloaded"), "{error}");
}

#[test]
fn stream_event_delta_is_provisional_text_delta() {
    let lines = parse_stream_json(
        r#"{"v":1,"type":"stream_event","session_id":"s1","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hel"}}}"#,
    )
    .unwrap();
    assert_eq!(lines.len(), 1);
    let events = to_desktop_events(&lines[0]);
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::TextDelta { text } => assert_eq!(text, "hel"),
        other => panic!("stream_event delta must project to TextDelta, got: {other:?}"),
    }
}

#[test]
fn stream_event_without_delta_is_surfaced_not_dropped() {
    let lines = parse_stream_json(
        r#"{"v":1,"type":"stream_event","session_id":"s1","event":{"type":"other","x":1}}"#,
    )
    .unwrap();
    let events = to_desktop_events(&lines[0]);
    assert_eq!(events.len(), 1);
    match &events[0] {
        DesktopEvent::Unknown { variant } => {
            assert!(variant.contains("stream_event"), "{variant}")
        }
        other => panic!("stream_event without delta must surface as Unknown, got: {other:?}"),
    }
}
